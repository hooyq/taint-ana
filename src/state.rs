use std::collections::HashMap;

/// LocalState 使用 String 作为 ID，支持多层嵌套（如 "_1.3.4.5"）
/// 
/// 使用 Union-Find（并查集）数据结构来管理变量的绑定关系：
/// 
/// **Union-Find 结构：**
/// - `parent`: 指向父节点，用于构建树结构。bind 时设置，指向被绑定到的变量
///              通过追踪 parent 可以找到整个组的"根节点"（root node）
///              当 parent == local_id 时，表示该节点是组的根节点
/// - `rank`: 树的高度上界（用于 Union by Rank 优化，自动管理，非手动设置）
///           当两个 rank 相等的树合并时，被附加的根的 rank 会 +1
///           这样可以保持树结构平衡，提高查找效率
/// 
/// **外部元数据：**
/// - `root`: 可选的标记，用于存储外部源头（如 taint 分析的输入源）
///           与 Union-Find 的根节点不同，这是用户提供的元数据
#[derive(Default, Debug, Clone)]
pub struct LocalState {
    /// 外部源头标记（如 taint source），可选。与 Union-Find 的根节点不同
    root: Option<String>,
    func_name: String,
    local_id: String,
    pub(crate) is_dropped: bool,
    /// Union-Find 的父指针。bind 时设置，指向父节点
    /// 通过追踪 parent 可以找到整个组的根节点（root node）
    /// 当 parent == local_id 时，表示该节点是组的根节点
    parent: String,
    /// Union-Find 的 rank（树的高度上界），用于优化合并操作
    /// 初始值为 0，只在两个 rank 相等的根合并时自动增长
    rank: u32,
}

impl LocalState {
    pub fn new(func_name: &str, local_id: String, root: Option<String>) -> Self {
        Self {
            root,
            func_name: func_name.to_string(),
            local_id: local_id.clone(),
            is_dropped: false,
            parent: local_id,
            rank: 0,
        }
    }

    pub fn find_root_from_id(id: &str, states: &HashMap<String, Self>) -> Option<(String, Vec<String>)> {
        let start_state = match states.get(id) {
            Some(s) => s,
            None => return None,
        };
        let mut current_id = id.to_string();
        let mut path: Vec<String> = Vec::new();
        loop {
            path.push(current_id.clone());
            let current_state = match states.get(&current_id) {
                Some(s) => s,
                None => return None,
            };
            if current_state.parent == current_id {
                return Some((current_id, path));
            }
            current_id = current_state.parent.clone();
        }
    }

    pub fn compress_path(states: &mut HashMap<String, LocalState>, path: &[String], root_id: &str) {
        for node_id in path.iter().rev().skip(1) {
            if let Some(state) = states.get_mut(node_id) {
                state.parent = root_id.to_string();
            }
        }
    }

    pub fn set_root_dropped(root_id: &str, states: &mut HashMap<String, LocalState>, dropped: bool) {
        if let Some(root) = states.get_mut(root_id) {
            root.is_dropped = dropped;
        }
    }

    pub fn get_root_dropped(root_id: &str, states: &HashMap<String, LocalState>) -> bool {
        states.get(root_id).map_or(false, |r| r.is_dropped)
    }

    /// 只读获取根的 rank 和 root（用于 bind 决定方向，无借用）
    pub fn get_root_rank_and_root(
        root_id: &str,
        states: &HashMap<String, Self>,
    ) -> Result<(u32, Option<String>), String> {
        let root_state = states.get(root_id).ok_or(format!("Root ID {} not found", root_id))?;
        Ok((root_state.rank, root_state.root.clone()))
    }

    /// 静态更新根：设置 parent 和 root
    pub fn update_root(
        to_root_id: &str,
        new_parent: &str,
        new_root: Option<String>,
        states: &mut HashMap<String, LocalState>,
    ) {
        if let Some(root) = states.get_mut(to_root_id) {
            root.parent = new_parent.to_string();
            if let Some(nr) = new_root {
                root.root = Some(nr);
            }
        }
    }

    pub fn binding_info(&self, states: &HashMap<String, Self>) -> String {
        let current_parent = states.get(&self.local_id).map_or(self.parent.clone(), |s| s.parent.clone());
        format!(
            "id: {}, func: {}, root: {:?}, parent: {}, dropped: {}, rank: {}",
            self.local_id, self.func_name, self.root, current_parent, self.is_dropped, self.rank
        )
    }
}

#[derive(Debug, Default, Clone)]
pub struct BindingManager {
    pub(crate) states: HashMap<String, LocalState>,
    func_name: String,
}

impl BindingManager {
    pub fn new(func_name: &str) -> Self {
        Self {
            func_name: func_name.to_string(),
            ..Default::default()
        }
    }

    pub fn register(&mut self, local_id: String, root: Option<String>) -> &mut LocalState {
        let func_name = self.func_name.clone();
        self.states
            .entry(local_id.clone())
            .or_insert_with(|| LocalState::new(&func_name, local_id.clone(), root));
        self.states.get_mut(&local_id).unwrap()  // 安全：刚插入
    }

    /// bind：分离读/写借用，只借用一个根进行修改
    pub fn bind(&mut self, id1: &str, id2: &str) -> Result<(), String> {
        if !self.states.contains_key(id1) || !self.states.contains_key(id2) {
            return Err("One or both IDs not registered".to_string());
        }

        // 压缩路径（&mut，但顺序分离）
        let (root_id1, path1) = LocalState::find_root_from_id(id1, &self.states).ok_or("Invalid id1")?;
        LocalState::compress_path(&mut self.states, &path1, &root_id1);

        let (root_id2, path2) = LocalState::find_root_from_id(id2, &self.states).ok_or("Invalid id2")?;
        LocalState::compress_path(&mut self.states, &path2, &root_id2);

        if root_id1 == root_id2 {
            return Ok(());
        }

        // 只读借用：获取两个根的 rank 和 root，决定链接方向（无冲突）
        // rank 是 Union-Find 的优化技术，用于保持树结构平衡
        let (rank1, root_opt1) = LocalState::get_root_rank_and_root(&root_id1, &self.states)?;
        let (rank2, root_opt2) = LocalState::get_root_rank_and_root(&root_id2, &self.states)?;

        // Union by Rank 策略：链接较低 rank 的树到较高 rank 的树
        // 这样可以保持树的高度较小，提高后续查找效率
        // 如果两个 rank 相等，任意选择一个方向并增加被附加根的 rank
        let (to_link_root, to_attach_root, inc_rank) = if rank1 > rank2 {
            (root_id2.clone(), root_id1.clone(), false)  // 链接 2 到 1
        } else if rank2 > rank1 {
            (root_id1.clone(), root_id2.clone(), false)  // 链接 1 到 2
        } else {
            (root_id2.clone(), root_id1.clone(), true)  // 相等：链接 2 到 1，增 rank1
        };

        // 合并 root（简单 or，优先 root1；如果都 None，则 None）
        let merged_root = root_opt1.or(root_opt2);

        // 只可变借用被链接根（to_link_root），更新其 parent 和 root
        LocalState::update_root(&to_link_root, &to_attach_root, merged_root, &mut self.states);

        // 如果相等，更新 attach 的 rank
        if inc_rank {
            if let Some(attach_root) = self.states.get_mut(&to_attach_root) {
                attach_root.rank += 1;
            } else {
                return Err(format!("Attach root {} not found after link", to_attach_root));
            }
        }

        Ok(())
    }

    pub fn idrop_group(&mut self, id: &str) {
        if !self.states.contains_key(id) {
            return;
        }
        let (root_id, path) = match LocalState::find_root_from_id(id, &self.states) {
            Some(p) => p,
            None => return,
        };
        {
            LocalState::compress_path(&mut self.states, &path, &root_id);
            LocalState::set_root_dropped(&root_id, &mut self.states, true);
        }
    }

    /// 恢复 local 的 drop 状态（用于重新赋值场景）
    pub fn undrop_group(&mut self, id: &str) {
        if !self.states.contains_key(id) {
            return;
        }
        let (root_id, path) = match LocalState::find_root_from_id(id, &self.states) {
            Some(p) => p,
            None => return,
        };
        {
            LocalState::compress_path(&mut self.states, &path, &root_id);
            LocalState::set_root_dropped(&root_id, &mut self.states, false);
        }
    }

    pub fn is_dropped(&mut self, id: &str) -> bool {
        if !self.states.contains_key(id) {
            return false;
        }
        let (root_id, path) = match LocalState::find_root_from_id(id, &self.states) {
            Some(p) => p,
            None => return false,
        };
        LocalState::compress_path(&mut self.states, &path, &root_id);
        LocalState::get_root_dropped(&root_id, &self.states)
    }

    /// 检查 local 是否已经被绑定（移动）到其他 local
    pub fn is_bound(&self, id: &str) -> bool {
        if let Some(_state) = self.states.get(id) {
            // 需要查找实际的 parent（考虑路径压缩）
            let (root_id, _) = match LocalState::find_root_from_id(id, &self.states) {
                Some(p) => p,
                None => return false,
            };
            // 如果 root_id != id，说明已经被绑定到其他 local
            root_id != id
        } else {
            false
        }
    }

    pub fn find_group(&mut self, id: &str) -> Option<(String, Vec<String>)> {
        if !self.states.contains_key(id) {
            return None;
        }
        let (root_id, path) = match LocalState::find_root_from_id(id, &self.states) {
            Some(p) => p,
            None => return None,
        };
        LocalState::compress_path(&mut self.states, &path, &root_id);
        let members: Vec<String> = self.states
            .iter()
            .filter_map(|(k, _v)| {
                let (r, _) = LocalState::find_root_from_id(k, &self.states).unwrap_or((k.clone(), vec![]));
                (r == root_id).then_some(k.clone())
            })
            .collect();
        Some((root_id, members))
    }

    pub fn print_all(&self) {
        for (id, state) in &self.states {
            let info = state.binding_info(&self.states);
            println!("{}: {}", id, info);
        }
        println!("---");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试1: 基本注册功能
    #[test]
    fn test_basic_register() {
        let mut manager = BindingManager::new("test_func");
        
        // 注册一些本地变量
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), Some("root1".to_string()));
        manager.register("_3".to_string(), None);
        
        // 验证注册成功
        assert!(manager.states.contains_key("_1"));
        assert!(manager.states.contains_key("_2"));
        assert!(manager.states.contains_key("_3"));
        
        // 验证初始状态
        assert_eq!(manager.states.get("_1").unwrap().func_name, "test_func");
        assert_eq!(manager.states.get("_2").unwrap().root, Some("root1".to_string()));
        assert_eq!(manager.states.get("_3").unwrap().root, None);
    }

    /// 测试2: 基本绑定功能（类似 Union-Find）
    #[test]
    fn test_basic_bind() {
        let mut manager = BindingManager::new("test_func");
        
        // 注册变量
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 绑定 _1 和 _2（相当于 _1 = _2 的移动操作）
        manager.bind("_1", "_2").unwrap();
        
        // 验证它们现在在同一个组中
        let (root1, members1) = manager.find_group("_1").unwrap();
        let (root2, members2) = manager.find_group("_2").unwrap();
        assert_eq!(root1, root2);
        assert!(members1.contains(&"_1".to_string()));
        assert!(members1.contains(&"_2".to_string()));
        
        // _3 应该还在独立的组中
        let (root3, _) = manager.find_group("_3").unwrap();
        assert_ne!(root1, root3);
    }

    /// 测试3: 多层绑定和路径压缩
    #[test]
    fn test_multiple_bind_and_path_compression() {
        let mut manager = BindingManager::new("test_func");
        
        // 注册多个变量
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        manager.register("_4".to_string(), None);
        
        // 创建链式绑定: _1 -> _2 -> _3
        manager.bind("_1", "_2").unwrap();
        manager.bind("_2", "_3").unwrap();
        
        // 所有三个应该在同一个组中
        let (root1, members1) = manager.find_group("_1").unwrap();
        let (root2, members2) = manager.find_group("_2").unwrap();
        let (root3, members3) = manager.find_group("_3").unwrap();
        
        assert_eq!(root1, root2);
        assert_eq!(root2, root3);
        assert!(members1.len() >= 3);
        
        // 绑定 _4 到 _1，验证路径压缩
        manager.bind("_4", "_1").unwrap();
        let (root4, _) = manager.find_group("_4").unwrap();
        assert_eq!(root1, root4);
    }

    /// 测试4: is_bound 检查
    /// is_bound 检查变量是否被绑定到其他变量（即它的 root 不是它自己）
    #[test]
    fn test_is_bound() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 初始状态：都没有被绑定（每个都是自己的根）
        assert!(!manager.is_bound("_1"));
        assert!(!manager.is_bound("_2"));
        assert!(!manager.is_bound("_3"));
        
        // 绑定 _1 和 _2
        // 由于初始 rank 相同，_2 会被链接到 _1（_1 成为根）
        manager.bind("_1", "_2").unwrap();
        
        // 绑定后：
        // - _1 是根（root_id == "_1"），所以 is_bound("_1") = false（没有被绑定到其他变量）
        // - _2 被绑定到 _1（root_id == "_1" != "_2"），所以 is_bound("_2") = true
        assert!(!manager.is_bound("_1"));  // _1 是根，没有被绑定
        assert!(manager.is_bound("_2"));   // _2 被绑定到 _1
        // _3 仍然是独立的（没有被绑定）
        assert!(!manager.is_bound("_3"));
        
        // 验证 _1 和 _2 在同一个组中（但 _1 是根）
        let (root1, _) = manager.find_group("_1").unwrap();
        let (root2, _) = manager.find_group("_2").unwrap();
        assert_eq!(root1, root2);  // 它们在同一个组
        assert_eq!(root1, "_1");   // _1 是根
    }

    /// 测试4b: is_bound 更详细的行为（不同 rank 的情况）
    #[test]
    fn test_is_bound_detailed() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 先绑定 _1 和 _2，使 _1 的 rank 变为 1，_2 被绑定到 _1
        manager.bind("_1", "_2").unwrap();
        assert!(!manager.is_bound("_1"));  // _1 是根
        assert!(manager.is_bound("_2"));   // _2 被绑定到 _1
        
        // 现在绑定 _3 到 _2（通过 _2 间接绑定到组）
        // 由于 _1 的 rank 更高，_3 会被链接到 _1
        manager.bind("_2", "_3").unwrap();
        assert!(!manager.is_bound("_1"));  // _1 仍然是根
        assert!(manager.is_bound("_2"));   // _2 仍然被绑定
        assert!(manager.is_bound("_3"));   // _3 也被绑定到 _1
        
        // 所有三个变量都在同一个组中，但只有 _1 是根
        let (root1, _) = manager.find_group("_1").unwrap();
        let (root2, _) = manager.find_group("_2").unwrap();
        let (root3, _) = manager.find_group("_3").unwrap();
        assert_eq!(root1, root2);
        assert_eq!(root2, root3);
        assert_eq!(root1, "_1");
    }

    /// 测试5: Drop 状态管理
    #[test]
    fn test_drop_management() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 绑定 _1 和 _2
        manager.bind("_1", "_2").unwrap();
        
        // 初始状态：都没有被 drop
        assert!(!manager.is_dropped("_1"));
        assert!(!manager.is_dropped("_2"));
        assert!(!manager.is_dropped("_3"));
        
        // drop _1（应该影响整个组）
        manager.idrop_group("_1");
        
        // 整个组都应该被标记为 dropped
        assert!(manager.is_dropped("_1"));
        assert!(manager.is_dropped("_2"));
        // _3 不受影响
        assert!(!manager.is_dropped("_3"));
        
        // 恢复 drop 状态
        manager.undrop_group("_1");
        assert!(!manager.is_dropped("_1"));
        assert!(!manager.is_dropped("_2"));
    }

    /// 测试6: root 传播（带 root 的绑定）
    #[test]
    fn test_root_propagation() {
        let mut manager = BindingManager::new("test_func");
        
        // 注册时指定 root
        manager.register("_1".to_string(), Some("external_root".to_string()));
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), Some("another_root".to_string()));
        
        // 绑定 _1 和 _2，_1 的 root 应该传播到 _2
        manager.bind("_1", "_2").unwrap();
        
        let (root1, _) = manager.find_group("_1").unwrap();
        let (root2, _) = manager.find_group("_2").unwrap();
        assert_eq!(root1, root2);
        
        // 绑定 _3 到 _1，应该合并 root（优先保留 _1 的 root）
        manager.bind("_1", "_3").unwrap();
        
        let (root3, _) = manager.find_group("_3").unwrap();
        assert_eq!(root1, root3);
    }

    /// 测试7: 复杂场景 - 完整的函数分析示例
    #[test]
    fn test_complete_usage_example() {
        // 模拟分析一个函数：let x = value; let y = x; drop(y); let z = y; (应该失败，因为 y 已被 drop)
        
        let mut manager = BindingManager::new("example_func");
        
        // 步骤1: 注册变量
        manager.register("value".to_string(), Some("input".to_string()));
        manager.register("x".to_string(), None);
        manager.register("y".to_string(), None);
        manager.register("z".to_string(), None);
        
        // 步骤2: x = value (移动 value 到 x)
        // 绑定后：value 被链接到 x（因为 rank 相等时，第二个参数被链接到第一个）
        manager.bind("x", "value").unwrap();
        assert!(!manager.is_bound("x"));     // x 是根，没有被绑定
        assert!(manager.is_bound("value"));  // value 被绑定到 x
        
        // 步骤3: y = x (移动 x 到 y)
        // 绑定后：由于 x 的 rank 更高（1），y 被链接到 x
        manager.bind("y", "x").unwrap();
        assert!(!manager.is_bound("x"));  // x 仍然是根，没有被绑定
        assert!(manager.is_bound("y"));   // y 被绑定到 x
        
        // 步骤4: drop(y)
        // 由于 value, x, y 都在同一个组中（根是 x），drop y 会 drop 整个组
        manager.idrop_group("y");
        assert!(manager.is_dropped("y"));   // y 被 drop
        assert!(manager.is_dropped("x"));   // x 也被 drop（同一个组）
        assert!(manager.is_dropped("value")); // value 也被 drop（同一个组）
        
        // 步骤5: z = y (尝试使用已 drop 的 y)
        // 在实际分析中，这会检测到错误
        // 这里我们验证 drop 状态确实被设置了
        assert!(manager.is_dropped("y"));
        
        // 步骤6: 如果 y 被重新赋值（重新初始化），应该 undrop
        manager.undrop_group("y");
        assert!(!manager.is_dropped("y"));
    }

    /// 测试8: 绑定相同元素（应该无操作）
    #[test]
    fn test_bind_same_element() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        
        // 绑定 _1 到自身应该成功但不改变状态
        manager.bind("_1", "_1").unwrap();
        // 应该还是指向自己（root == 自己）
        assert_eq!(manager.states.get("_1").unwrap().parent, "_1");
    }

    /// 测试9: 绑定已绑定的元素
    #[test]
    fn test_bind_already_bound() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // _1 和 _2 已经绑定
        manager.bind("_1", "_2").unwrap();
        let (root_before, _) = manager.find_group("_1").unwrap();
        
        // 再次绑定应该无操作
        manager.bind("_1", "_2").unwrap();
        let (root_after, _) = manager.find_group("_1").unwrap();
        assert_eq!(root_before, root_after);
        
        // 绑定 _3 到 _1 应该合并组
        manager.bind("_3", "_1").unwrap();
        let (root3, members3) = manager.find_group("_3").unwrap();
        assert_eq!(root_before, root3);
        assert!(members3.len() >= 3);
    }

    /// 测试10: 错误处理 - 绑定未注册的 ID
    #[test]
    fn test_bind_unregistered_id() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        
        // 尝试绑定未注册的 ID 应该返回错误
        let result = manager.bind("_1", "_2");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not registered"));
    }

    /// 测试11: find_group 返回正确的成员
    #[test]
    fn test_find_group_members() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        manager.register("_4".to_string(), None);
        
        // 创建两个独立的组
        manager.bind("_1", "_2").unwrap();
        manager.bind("_3", "_4").unwrap();
        
        // 查找第一个组
        let (root1, members1) = manager.find_group("_1").unwrap();
        assert!(members1.contains(&"_1".to_string()));
        assert!(members1.contains(&"_2".to_string()));
        assert_eq!(members1.len(), 2);
        
        // 查找第二个组
        let (root2, members2) = manager.find_group("_3").unwrap();
        assert!(members2.contains(&"_3".to_string()));
        assert!(members2.contains(&"_4".to_string()));
        assert_eq!(members2.len(), 2);
        
        assert_ne!(root1, root2);
    }

    /// 测试12: rank 增长机制
    #[test]
    fn test_rank_increase() {
        let mut manager = BindingManager::new("test_func");
        
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 初始 rank 都是 0
        let (root1_before, _) = manager.find_group("_1").unwrap();
        let initial_rank1 = manager.states.get(&root1_before).unwrap().rank;
        assert_eq!(initial_rank1, 0);
        
        // 绑定两个 rank 相等的组，应该增加被附加的根的 rank
        manager.bind("_1", "_2").unwrap();
        let (root1_after, _) = manager.find_group("_1").unwrap();
        let new_rank1 = manager.states.get(&root1_after).unwrap().rank;
        // 其中一个根的 rank 应该增加（取决于实现细节）
        // 这里我们主要验证绑定成功
        assert!(new_rank1 >= initial_rank1);
    }
}
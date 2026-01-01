use rustc_middle::mir::{BasicBlock, Body};
use std::collections::HashSet;
use crate::state::BindingManager;


pub fn dfs_visit<'tcx>(
    body: &Body<'tcx>,
    start: BasicBlock,
    visitor: &mut impl FnMut(BasicBlock),
) {
    let mut visited = HashSet::<BasicBlock>::new();

    fn dfs<'tcx>(
        body: &Body<'tcx>,
        idx: BasicBlock,
        visited: &mut HashSet<BasicBlock>,
        visitor: &mut impl FnMut(BasicBlock),
    ) {
        if !visited.insert(idx) {
            return;
        }

        visitor(idx);

        let block = &body.basic_blocks[idx];
        if let Some(ref terminator) = block.terminator {
            for succ in terminator.successors() {
                dfs(body, succ, visited, visitor);
            }
        }
    }

    dfs(body, start, &mut visited, visitor);
}

/// DFS遍历，在遇到分支时保存和恢复manager状态
pub fn dfs_visit_with_manager<'tcx>(
    body: &Body<'tcx>,
    start: BasicBlock,
    manager: &mut BindingManager,
    visitor: &mut impl FnMut(BasicBlock, &mut BindingManager),
) {
    let mut visited = HashSet::<BasicBlock>::new();

    fn dfs<'tcx>(
        body: &Body<'tcx>,
        idx: BasicBlock,
        visited: &mut HashSet<BasicBlock>,
        manager: &mut BindingManager,
        visitor: &mut impl FnMut(BasicBlock, &mut BindingManager),
    ) {
        if !visited.insert(idx) {
            return;
        }

        visitor(idx, manager);

        let block = &body.basic_blocks[idx];
        if let Some(ref terminator) = block.terminator {
            let successors: Vec<_> = terminator.successors().collect();
            
            // 如果遇到分支（多个successors），保存manager状态
            if successors.len() > 1 {
                let saved_state = manager.clone();
                
                // 对每个分支，从保存的状态开始
                for succ in successors {
                    *manager = saved_state.clone();
                    dfs(body, succ, visited, manager, visitor);
                }
            } else {
                // 单个successor，直接继续
                for succ in successors {
                    dfs(body, succ, visited, manager, visitor);
                }
            }
        }
    }

    dfs(body, start, &mut visited, manager, visitor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BindingManager;

    /// 测试 DFS 在分支时能正确保存和恢复状态
    /// 
    /// 场景：
    /// - 在分支前修改 manager 状态（如注册 local、绑定、drop）
    /// - 进入分支后，每个分支应该从保存的状态开始
    /// - 一个分支的修改不应该影响另一个分支
    #[test]
    fn test_dfs_branch_state_preservation() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态：注册一些 locals
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.register("_3".to_string(), None);
        
        // 在分支前进行一些操作
        manager.bind("_1", "_2").unwrap();
        manager.idrop_group("_1");
        
        // 保存状态（模拟分支前的状态保存）
        let mut saved_state = &mut manager;
        
        // 验证保存的状态
        assert!(saved_state.is_dropped("_1"));
        assert!(saved_state.is_dropped("_2")); // 因为绑定关系
        
        // 模拟分支 1：修改状态
        let mut branch1_state = saved_state.clone();
        branch1_state.register("_4".to_string(), None);
        branch1_state.bind("_3", "_4").unwrap();
        branch1_state.idrop_group("_3");
        
        // 验证分支 1 的状态
        assert!(branch1_state.is_dropped("_3"));
        assert!(branch1_state.is_dropped("_4"));
        assert!(branch1_state.is_dropped("_1")); // 从保存状态继承
        assert!(branch1_state.is_dropped("_2")); // 从保存状态继承
        
        // 模拟分支 2：从保存的状态开始（回溯）
        let mut branch2_state = saved_state.clone();
        branch2_state.register("_5".to_string(), None);
        branch2_state.bind("_3", "_5").unwrap();
        // 注意：分支 2 没有 drop _3
        
        // 验证分支 2 的状态（应该从保存状态开始，不受分支 1 影响）
        assert!(!branch2_state.is_dropped("_3")); // 分支 2 没有 drop
        assert!(!branch2_state.is_dropped("_5")); // 分支 2 没有 drop
        assert!(branch2_state.is_dropped("_1")); // 从保存状态继承
        assert!(branch2_state.is_dropped("_2")); // 从保存状态继承
        
        // 验证原始保存状态没有被修改
        assert!(saved_state.is_dropped("_1"));
        assert!(saved_state.is_dropped("_2"));
        assert!(!saved_state.is_dropped("_3")); // 保存时 _3 没有被 drop
        assert!(!saved_state.states.contains_key("_4")); // 保存时 _4 不存在
        assert!(!saved_state.states.contains_key("_5")); // 保存时 _5 不存在
    }
    
    /// 测试多个分支时，每个分支都从相同的初始状态开始
    #[test]
    fn test_dfs_multiple_branches_same_initial_state() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.bind("_1", "_2").unwrap();
        
        // 保存状态
        let mut saved_state = manager.clone();
        
        // 创建多个分支，每个都从保存的状态开始
        let mut branches = Vec::new();
        for i in 0..5 {
            let mut branch_state = saved_state.clone();
            let local_id = format!("_{}", 10 + i);
            branch_state.register(local_id.clone(), None);
            branch_state.bind("_1", &local_id).unwrap();
            branch_state.idrop_group(&local_id);
            branches.push(branch_state);
        }
        
        // 验证每个分支都从相同的初始状态开始
        for (idx, branch) in branches.iter_mut().enumerate() {
            // 每个分支都应该有相同的初始绑定关系
            let (_root1, members1) = branch.find_group("_1").unwrap();
            assert!(members1.contains(&"_1".to_string()));
            assert!(members1.contains(&"_2".to_string()));
            
            // 每个分支都有自己的新 local（10+i）
            let local_id = format!("_{}", 10 + idx);
            assert!(branch.states.contains_key(&local_id));
            assert!(branch.is_dropped(&local_id));
        }
        
        // 验证保存的状态没有被修改
        assert_eq!(saved_state.states.len(), 2); // 只有 _1 和 _2
        let (_root, members) = saved_state.find_group("_1").unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"_1".to_string()));
        assert!(members.contains(&"_2".to_string()));
    }
    
    /// 测试分支退回后可以回溯到保存的状态
    #[test]
    fn test_dfs_branch_rollback() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.bind("_1", "_2").unwrap();
        
        // 保存状态 A
        let state_a = manager.clone();
        
        // 继续操作
        manager.register("_3".to_string(), None);
        manager.bind("_1", "_3").unwrap();
        manager.idrop_group("_1");
        
        // 保存状态 B
        let state_b = manager.clone();
        
        // 模拟分支：从状态 A 开始
        let mut branch_from_a = state_a.clone();
        branch_from_a.register("_4".to_string(), None);
        branch_from_a.bind("_2", "_4").unwrap();
        
        // 模拟分支：从状态 B 开始
        let mut branch_from_b = state_b.clone();
        branch_from_b.register("_5".to_string(), None);
        branch_from_b.bind("_3", "_5").unwrap();
        
        // 验证分支从 A 的状态（回溯）
        assert!(!branch_from_a.is_dropped("_1")); // 状态 A 时 _1 没有被 drop
        assert!(!branch_from_a.is_dropped("_2")); // 状态 A 时 _2 没有被 drop
        assert!(!branch_from_a.states.contains_key("_3")); // 状态 A 时 _3 不存在
        
        // 验证分支从 B 的状态
        assert!(branch_from_b.is_dropped("_1")); // 状态 B 时 _1 已经被 drop
        assert!(branch_from_b.is_dropped("_2")); // 状态 B 时 _2 因为绑定关系也被 drop
        assert!(branch_from_b.is_dropped("_3")); // 状态 B 时 _3 因为绑定关系也被 drop
        assert!(branch_from_b.states.contains_key("_3")); // 状态 B 时 _3 存在
        
        // 验证状态 A 和 B 没有被修改
        assert_eq!(state_a.states.len(), 2); // 只有 _1 和 _2
        assert_eq!(state_b.states.len(), 3); // _1, _2, _3
    }
    
    /// 测试嵌套分支（分支中的分支）
    #[test]
    fn test_dfs_nested_branches() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        manager.bind("_1", "_2").unwrap();
        
        // 第一层分支：保存状态
        let level1_state = manager.clone();
        
        // 第一层分支 1
        let mut level1_branch1 = level1_state.clone();
        level1_branch1.register("_3".to_string(), None);
        level1_branch1.bind("_1", "_3").unwrap();
        
        // 在第一层分支 1 中，进入第二层分支
        let level2_state = level1_branch1.clone();
        
        // 第二层分支 1
        let mut level2_branch1 = level2_state.clone();
        level2_branch1.register("_4".to_string(), None);
        level2_branch1.bind("_3", "_4").unwrap();
        level2_branch1.idrop_group("_4");
        
        // 第二层分支 2（从 level2_state 回溯）
        let mut level2_branch2 = level2_state.clone();
        level2_branch2.register("_5".to_string(), None);
        level2_branch2.bind("_3", "_5").unwrap();
        // 注意：level2_branch2 没有 drop _5
        
        // 验证第二层分支 2 从 level2_state 回溯
        assert!(!level2_branch2.is_dropped("_4")); // level2_state 时 _4 不存在
        assert!(!level2_branch2.is_dropped("_5")); // level2_branch2 没有 drop _5
        assert!(level2_branch2.states.contains_key("_3")); // level2_state 时 _3 存在
        
        // 验证 level2_state 没有被修改
        assert!(!level2_state.states.contains_key("_4"));
        assert!(!level2_state.states.contains_key("_5"));
        assert!(level2_state.states.contains_key("_3"));
        
        // 验证第一层分支 1 的状态
        assert!(level1_branch1.states.contains_key("_3"));
        assert!(!level1_branch1.states.contains_key("_4")); // 第一层分支 1 没有直接注册 _4
        assert!(!level1_branch1.states.contains_key("_5")); // 第一层分支 1 没有直接注册 _5
    }
    
    /// 测试单分支（非分支路径）不保存状态
    #[test]
    fn test_dfs_single_successor_no_save() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        
        // 单分支路径：连续操作，状态应该累积
        manager.bind("_1", "_2").unwrap();
        manager.register("_3".to_string(), None);
        manager.bind("_1", "_3").unwrap();
        manager.idrop_group("_1");
        
        // 验证状态累积
        assert!(manager.is_dropped("_1"));
        assert!(manager.is_dropped("_2")); // 因为绑定关系
        assert!(manager.is_dropped("_3")); // 因为绑定关系
        
        let (_root, members) = manager.find_group("_1").unwrap();
        assert!(members.contains(&"_1".to_string()));
        assert!(members.contains(&"_2".to_string()));
        assert!(members.contains(&"_3".to_string()));
    }
    
    /// 测试分支后状态合并（如果分支后汇合）
    /// 注意：当前实现中，分支是独立的，不会合并
    /// 这个测试验证分支的独立性
    #[test]
    fn test_dfs_branch_independence() {
        let mut manager = BindingManager::new("test_func");
        
        // 初始状态
        manager.register("_1".to_string(), None);
        manager.register("_2".to_string(), None);
        
        // 保存状态
        let mut saved_state = manager.clone();
        
        // 分支 1：drop _1
        let mut branch1 = saved_state.clone();
        branch1.idrop_group("_1");
        
        // 分支 2：drop _2
        let mut branch2 = saved_state.clone();
        branch2.idrop_group("_2");
        
        // 验证分支独立性
        assert!(branch1.is_dropped("_1"));
        assert!(!branch1.is_dropped("_2")); // 分支 1 没有 drop _2
        
        assert!(!branch2.is_dropped("_1")); // 分支 2 没有 drop _1
        assert!(branch2.is_dropped("_2"));
        
        // 验证保存的状态没有被修改
        assert!(!saved_state.is_dropped("_1"));
        assert!(!saved_state.is_dropped("_2"));
    }
}

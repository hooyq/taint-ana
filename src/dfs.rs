use rustc_middle::mir::{BasicBlock, Body};
use std::collections::{HashSet, HashMap};
use crate::state::BindingManager;

/// DFS 配置结构体，控制遍历行为
#[derive(Clone, Debug)]
pub struct DfsConfig {
    /// 记录的前序节点数量
    /// - 0: 和现有逻辑一样，每个 block 只访问一次
    /// - k > 0: 记录最近 k 个前序 block，路径不同可以重复访问
    pub k_predecessor: usize,
    
    /// 单个 block 的最大访问次数（防止无限循环）
    pub max_visits_per_block: usize,
}

impl Default for DfsConfig {
    fn default() -> Self {
        Self {
            k_predecessor: 2,
            max_visits_per_block: 10,  // 默认最多访问 10 次
        }
    }
}

/// 路径上下文结构体，记录当前遍历的路径信息
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PathContext {
    /// 最近 k 个前序 BasicBlock（队列，保持顺序）
    predecessors: Vec<BasicBlock>,
}

impl PathContext {
    pub fn new(k: usize) -> Self {
        Self { 
            predecessors: Vec::with_capacity(k) 
        }
    }
    
    /// 添加新的 block 到路径，保持最近 k 个
    pub fn push(&mut self, block: BasicBlock, k: usize) {
        self.predecessors.push(block);
        if self.predecessors.len() > k {
            self.predecessors.remove(0);
        }
    }
    
    /// 获取最近 k 个前序（用于 visited 检查）
    pub fn get_key(&self) -> Vec<BasicBlock> {
        self.predecessors.clone()
    }
}

/// 访问状态管理结构体
pub struct VisitState {
    /// 记录 (block, k_predecessors) 的访问情况
    visited_paths: HashSet<(BasicBlock, Vec<BasicBlock>)>,
    
    /// 记录每个 block 的总访问次数
    visit_counts: HashMap<BasicBlock, usize>,
    
    /// 配置
    config: DfsConfig,
    
    /// 统计信息
    stats: DfsStats,
}

/// DFS 遍历统计信息
#[derive(Clone, Debug, Default)]
pub struct DfsStats {
    /// 总访问次数（包括被跳过的）
    pub total_visit_attempts: usize,
    
    /// 成功访问次数
    pub successful_visits: usize,
    
    /// 因路径重复被跳过的次数
    pub skipped_duplicate_path: usize,
    
    /// 因达到访问上限被跳过的次数
    pub skipped_max_visits: usize,
    
    /// 访问过的唯一路径数量
    pub unique_paths: usize,
    
    /// 访问过的唯一 block 数量
    pub unique_blocks: usize,
}

impl VisitState {
    pub fn new(config: DfsConfig) -> Self {
        Self {
            visited_paths: HashSet::new(),
            visit_counts: HashMap::new(),
            config,
            stats: DfsStats::default(),
        }
    }
    
    /// 检查是否应该访问该 block
    /// 返回 true 表示可以访问，false 表示应该跳过
    pub fn should_visit(&mut self, block: BasicBlock, context: &PathContext) -> bool {
        self.stats.total_visit_attempts += 1;
        
        // 检查 1: 访问次数是否超过上限
        let count = self.visit_counts.get(&block).copied().unwrap_or(0);
        if count >= self.config.max_visits_per_block {
            self.stats.skipped_max_visits += 1;
            return false;
        }
        
        // 检查 2: (block, predecessors) 组合是否已访问
        let key = if self.config.k_predecessor == 0 {
            // k=0 时，只检查 block 本身（兼容旧逻辑）
            (block, vec![])
        } else {
            // k>0 时，检查 (block, 最近k个前序) 组合
            (block, context.get_key())
        };
        
        if self.visited_paths.contains(&key) {
            self.stats.skipped_duplicate_path += 1;
            return false;
        }
        
        // 标记为已访问
        self.visited_paths.insert(key);
        *self.visit_counts.entry(block).or_insert(0) += 1;
        
        // 更新统计信息
        self.stats.successful_visits += 1;
        self.stats.unique_paths = self.visited_paths.len();
        self.stats.unique_blocks = self.visit_counts.len();
        
        true
    }
    
    /// 获取统计信息
    pub fn get_stats(&self) -> &DfsStats {
        &self.stats
    }
    
    /// 打印统计信息
    pub fn print_stats(&self, func_name: &str) {
        if std::env::var("TAINT_ANA_DFS_STATS").is_ok() {
            println!("\n=== DFS Statistics for {} ===", func_name);
            println!("  Config: k={}, max_visits={}", 
                     self.config.k_predecessor, 
                     self.config.max_visits_per_block);
            println!("  Total visit attempts: {}", self.stats.total_visit_attempts);
            println!("  Successful visits: {}", self.stats.successful_visits);
            println!("  Skipped (duplicate path): {}", self.stats.skipped_duplicate_path);
            println!("  Skipped (max visits): {}", self.stats.skipped_max_visits);
            println!("  Unique paths explored: {}", self.stats.unique_paths);
            println!("  Unique blocks visited: {}", self.stats.unique_blocks);
            
            // 计算路径爆炸因子
            if self.stats.unique_blocks > 0 {
                let explosion_factor = self.stats.unique_paths as f64 / self.stats.unique_blocks as f64;
                println!("  Path explosion factor: {:.2}x", explosion_factor);
            }
            println!("================================\n");
        }
    }
}


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

/// 增强版 DFS 遍历，支持 k-predecessor 路径敏感性
/// 
/// # 参数
/// - `body`: MIR body
/// - `start`: 起始 BasicBlock
/// - `manager`: 绑定管理器（在分支时会保存和恢复状态）
/// - `config`: DFS 配置（k 值、最大访问次数等）
/// - `visitor`: 访问器函数，接收 (BasicBlock, &mut BindingManager, &PathContext)
/// 
/// # 返回
/// 返回 DFS 遍历的统计信息
pub fn dfs_visit_with_manager_ex<'tcx>(
    body: &Body<'tcx>,
    start: BasicBlock,
    manager: &mut BindingManager,
    config: DfsConfig,
    visitor: &mut impl FnMut(BasicBlock, &mut BindingManager, &PathContext),
) -> DfsStats {
    let mut visit_state = VisitState::new(config.clone());
    let mut path_context = PathContext::new(config.k_predecessor);
    
    fn dfs<'tcx>(
        body: &Body<'tcx>,
        idx: BasicBlock,
        visit_state: &mut VisitState,
        path_context: &mut PathContext,
        manager: &mut BindingManager,
        config: &DfsConfig,
        visitor: &mut impl FnMut(BasicBlock, &mut BindingManager, &PathContext),
    ) {
        // 关键改进：基于路径上下文判断是否访问
        if !visit_state.should_visit(idx, path_context) {
            return;
        }
        
        // 调用访问函数
        visitor(idx, manager, path_context);
        
        let block = &body.basic_blocks[idx];
        if let Some(ref terminator) = block.terminator {
            let successors: Vec<_> = terminator.successors().collect();
            
            // 分支处理（保存状态）
            if successors.len() > 1 {
                let saved_manager = manager.clone();
                
                for succ in successors {
                    // 每个分支从保存的状态开始
                    *manager = saved_manager.clone();
                    
                    // 更新路径上下文（添加当前 block）
                    let mut new_context = path_context.clone();
                    new_context.push(idx, config.k_predecessor);
                    
                    dfs(body, succ, visit_state, &mut new_context, manager, config, visitor);
                }
            } else {
                // 单后继：直接继续，更新路径上下文
                for succ in successors {
                    path_context.push(idx, config.k_predecessor);
                    dfs(body, succ, visit_state, path_context, manager, config, visitor);
                }
            }
        }
    }
    
    dfs(body, start, &mut visit_state, &mut path_context, manager, &config, visitor);
    
    // 返回统计信息
    visit_state.stats.clone()
}

/// DFS遍历，在遇到分支时保存和恢复manager状态
/// 
/// 这是兼容性包装函数，内部调用 `dfs_visit_with_manager_ex` with k=0
pub fn dfs_visit_with_manager<'tcx>(
    body: &Body<'tcx>,
    start: BasicBlock,
    manager: &mut BindingManager,
    visitor: &mut impl FnMut(BasicBlock, &mut BindingManager),
) {
    // 使用默认配置（k=0），保持原有行为
    let config = DfsConfig::default();
    
    dfs_visit_with_manager_ex(body, start, manager, config, &mut |bb, mgr, _ctx| {
        // 忽略 PathContext，调用原有的 visitor
        visitor(bb, mgr);
    });
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
    
    // ========== k-predecessor 路径敏感性测试 ==========
    
    /// 测试 k=0 时的行为（每个 block 只访问一次）
    #[test]
    fn test_k0_single_visit_per_block() {
        use rustc_middle::mir::BasicBlock;
        
        let config = DfsConfig {
            k_predecessor: 0,
            max_visits_per_block: 10,
        };
        
        let mut visit_state = VisitState::new(config);
        let mut context = PathContext::new(0);
        
        let bb0 = BasicBlock::from_usize(0);
        let bb1 = BasicBlock::from_usize(1);
        
        // 第一次访问 bb0：应该成功
        assert!(visit_state.should_visit(bb0, &context));
        
        // 第二次访问 bb0（即使路径不同）：应该失败
        context.push(bb1, 0);
        assert!(!visit_state.should_visit(bb0, &context));
    }
    
    /// 测试 k=1 时的路径敏感性
    #[test]
    fn test_k1_path_sensitivity() {
        use rustc_middle::mir::BasicBlock;
        
        let config = DfsConfig {
            k_predecessor: 1,
            max_visits_per_block: 10,
        };
        
        let mut visit_state = VisitState::new(config);
        
        let bb0 = BasicBlock::from_usize(0);
        let bb1 = BasicBlock::from_usize(1);
        let bb2 = BasicBlock::from_usize(2);
        let bb3 = BasicBlock::from_usize(3);
        
        // 路径 1: bb0 -> bb2
        let mut context1 = PathContext::new(1);
        context1.push(bb0, 1);
        assert!(visit_state.should_visit(bb2, &context1));
        
        // 路径 2: bb1 -> bb2 (不同的前序，应该可以再次访问 bb2)
        let mut context2 = PathContext::new(1);
        context2.push(bb1, 1);
        assert!(visit_state.should_visit(bb2, &context2));
        
        // 路径 3: bb0 -> bb2 (相同的前序，应该被跳过)
        let mut context3 = PathContext::new(1);
        context3.push(bb0, 1);
        assert!(!visit_state.should_visit(bb2, &context3));
    }
    
    /// 测试 k=2 时的路径敏感性
    #[test]
    fn test_k2_path_sensitivity() {
        use rustc_middle::mir::BasicBlock;
        
        let config = DfsConfig {
            k_predecessor: 2,
            max_visits_per_block: 10,
        };
        
        let mut visit_state = VisitState::new(config);
        
        let bb0 = BasicBlock::from_usize(0);
        let bb1 = BasicBlock::from_usize(1);
        let bb2 = BasicBlock::from_usize(2);
        let bb3 = BasicBlock::from_usize(3);
        
        // 路径 1: bb0 -> bb1 -> bb3
        let mut context1 = PathContext::new(2);
        context1.push(bb0, 2);
        context1.push(bb1, 2);
        assert!(visit_state.should_visit(bb3, &context1));
        
        // 路径 2: bb0 -> bb2 -> bb3 (不同的前序，应该可以再次访问 bb3)
        let mut context2 = PathContext::new(2);
        context2.push(bb0, 2);
        context2.push(bb2, 2);
        assert!(visit_state.should_visit(bb3, &context2));
        
        // 路径 3: bb1 -> bb2 -> bb3 (又一个不同的前序)
        let mut context3 = PathContext::new(2);
        context3.push(bb1, 2);
        context3.push(bb2, 2);
        assert!(visit_state.should_visit(bb3, &context3));
        
        // 路径 4: bb0 -> bb1 -> bb3 (相同的前序，应该被跳过)
        let mut context4 = PathContext::new(2);
        context4.push(bb0, 2);
        context4.push(bb1, 2);
        assert!(!visit_state.should_visit(bb3, &context4));
    }
    
    /// 测试最大访问次数限制
    #[test]
    fn test_max_visits_limit() {
        use rustc_middle::mir::BasicBlock;
        
        let config = DfsConfig {
            k_predecessor: 1,
            max_visits_per_block: 3,  // 最多访问 3 次
        };
        
        let mut visit_state = VisitState::new(config);
        
        let bb0 = BasicBlock::from_usize(0);
        let bb1 = BasicBlock::from_usize(1);
        
        // 使用不同的前序访问 bb1 三次
        for i in 0..3 {
            let mut context = PathContext::new(1);
            context.push(BasicBlock::from_usize(10 + i), 1);
            assert!(visit_state.should_visit(bb1, &context), "Visit {} should succeed", i);
        }
        
        // 第 4 次访问应该失败（即使前序不同）
        let mut context = PathContext::new(1);
        context.push(BasicBlock::from_usize(99), 1);
        assert!(!visit_state.should_visit(bb1, &context), "Visit 4 should fail due to max_visits limit");
    }
    
    /// 测试 PathContext 的 push 方法正确维护最近 k 个元素
    #[test]
    fn test_path_context_push() {
        use rustc_middle::mir::BasicBlock;
        
        let mut context = PathContext::new(3);
        
        // 添加 3 个元素
        context.push(BasicBlock::from_usize(0), 3);
        context.push(BasicBlock::from_usize(1), 3);
        context.push(BasicBlock::from_usize(2), 3);
        
        let key = context.get_key();
        assert_eq!(key.len(), 3);
        assert_eq!(key[0], BasicBlock::from_usize(0));
        assert_eq!(key[1], BasicBlock::from_usize(1));
        assert_eq!(key[2], BasicBlock::from_usize(2));
        
        // 添加第 4 个元素，应该移除第一个
        context.push(BasicBlock::from_usize(3), 3);
        let key = context.get_key();
        assert_eq!(key.len(), 3);
        assert_eq!(key[0], BasicBlock::from_usize(1));
        assert_eq!(key[1], BasicBlock::from_usize(2));
        assert_eq!(key[2], BasicBlock::from_usize(3));
    }
    
    /// 测试 k=0 时 PathContext 不记录任何前序
    #[test]
    fn test_k0_no_predecessors() {
        use rustc_middle::mir::BasicBlock;
        
        let mut context = PathContext::new(0);
        
        context.push(BasicBlock::from_usize(0), 0);
        context.push(BasicBlock::from_usize(1), 0);
        
        let key = context.get_key();
        assert_eq!(key.len(), 0, "k=0 should not record any predecessors");
    }


}

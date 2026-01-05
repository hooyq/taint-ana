# k-Predecessor 路径敏感 DFS 使用指南

## 概述

本项目实现了 k-predecessor 路径敏感的 DFS 遍历算法，用于提升 MIR 分析的精确度。通过记录最近 k 个前序 BasicBlock，可以在精确性和性能之间取得平衡。

## 核心概念

### 路径敏感性

- **k=0**：每个 BasicBlock 只访问一次（原有行为）
- **k>0**：记录最近 k 个前序 block，只有当 (block, predecessors) 组合重复时才跳过

### 示例

假设有如下控制流：

```
bb0 -> bb1 -> bb3
bb0 -> bb2 -> bb3
```

- **k=0**：bb3 只会被访问一次（第一条路径）
- **k=1**：bb3 会被访问两次，因为前序不同（bb1 vs bb2）
- **k=2**：同样访问两次，路径 [bb0, bb1] -> bb3 和 [bb0, bb2] -> bb3 不同

## 配置方式

直接在代码中配置（推荐方式）：

### 修改 `src/callbacks.rs` 中的配置

找到 `get_dfs_config()` 函数，修改返回的配置：

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 2,        // 修改这里：0=不敏感, 1-3=推荐, >3=高精度
        max_visits_per_block: 10, // 修改这里：防止无限循环
    }
}
```

### 启用统计信息输出

在 `analyze_function()` 函数中，取消注释这行：

```rust
// 找到这行并取消注释
print_dfs_stats(&name, &stats);
```

## 使用示例

### 示例 1：默认配置（当前设置为 k=2）

直接运行即可：

```bash
cargo run --manifest-path experiment/fn-signature-extractor/taintAna/Cargo.toml
```

### 示例 2：修改为不敏感模式（k=0）

修改 `src/callbacks.rs` 中的配置：

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 0,        // 改为 0
        max_visits_per_block: 10,
    }
}
```

然后运行：

```bash
cargo run --manifest-path experiment/fn-signature-extractor/taintAna/Cargo.toml
```

### 示例 3：启用统计信息

在 `src/callbacks.rs` 的 `analyze_function()` 中，取消注释：

```rust
// 取消注释这行
print_dfs_stats(&name, &stats);
```

统计信息输出示例：

```
=== DFS Statistics for example::main ===
  Total visit attempts: 45
  Successful visits: 32
  Skipped (duplicate path): 10
  Skipped (max visits): 3
  Unique paths explored: 32
  Unique blocks visited: 15
  Path explosion factor: 2.13x
================================
```

## 推荐配置

| 场景 | k 值 | max_visits | 说明 |
|------|------|------------|------|
| 快速粗略分析 | 0 | 10 | 默认配置，最快 |
| 平衡精度和性能 | 1-3 | 10-20 | **推荐**，适合大多数场景 |
| 复杂控制流分析 | 5-10 | 20-50 | 较高精度，性能开销较大 |
| 小函数精确分析 | 100+ | 100+ | 接近完全路径敏感 |

## 性能考虑

### 路径爆炸因子

路径爆炸因子 = 唯一路径数 / 唯一 block 数

- **< 1.5x**：路径爆炸不明显，可以增大 k 值
- **1.5x - 3x**：中等路径爆炸，当前配置合理
- **> 3x**：路径爆炸严重，建议减小 k 值或增大 max_visits

### 调优建议

1. **从小开始**：先用 k=1 或 k=2 测试
2. **观察统计**：启用 `TAINT_ANA_DFS_STATS` 查看路径爆炸情况
3. **逐步调整**：根据统计信息调整 k 值和 max_visits
4. **针对性配置**：对不同复杂度的代码使用不同配置

## API 使用

### 在代码中使用

```rust
use crate::dfs::{DfsConfig, dfs_visit_with_manager_ex};

// 创建配置
let config = DfsConfig {
    k_predecessor: 2,
    max_visits_per_block: 10,
};

// 使用增强版 DFS
let stats = dfs_visit_with_manager_ex(
    body,
    start_block,
    &mut manager,
    config,
    &mut |bb, mgr, ctx| {
        // 访问器函数
        // ctx.predecessors 包含最近 k 个前序 block
        println!("Visiting {:?}, predecessors: {:?}", bb, ctx.predecessors);
    },
);

// 检查统计信息
println!("Explored {} unique paths", stats.unique_paths);
```

### 兼容性

原有的 `dfs_visit_with_manager` 函数仍然可用，内部自动使用 k=0 配置：

```rust
use crate::dfs::dfs_visit_with_manager;

dfs_visit_with_manager(body, start_block, &mut manager, &mut |bb, mgr| {
    // 原有代码无需修改
});
```

## 测试

运行单元测试：

```bash
cargo test --manifest-path experiment/fn-signature-extractor/taintAna/Cargo.toml dfs
```

测试覆盖：
- k=0 行为验证
- k=1, k=2 路径敏感性
- 最大访问次数限制
- PathContext 维护逻辑
- 统计信息准确性

## 故障排查

### 问题：分析时间过长

**原因**：k 值过大导致路径爆炸

**解决**：
1. 减小 k 值（如从 5 降到 2）
2. 减小 max_visits（如从 50 降到 10）
3. 启用统计信息查看路径爆炸因子

### 问题：检测不到某些 bug

**原因**：k 值过小，路径敏感性不足

**解决**：
1. 增大 k 值（如从 1 增到 3）
2. 增大 max_visits 允许更多重复访问
3. 对特定函数使用更高的 k 值

### 问题：统计信息不显示

**原因**：未设置环境变量

**解决**：
```bash
export TAINT_ANA_DFS_STATS=1  # Linux/Mac
$env:TAINT_ANA_DFS_STATS=1    # Windows PowerShell
```

## 实现细节

### 核心数据结构

- `DfsConfig`：配置结构体
- `PathContext`：路径上下文，维护最近 k 个前序
- `VisitState`：访问状态管理，记录已访问路径
- `DfsStats`：统计信息

### 关键算法

1. **路径维护**：使用固定大小的 Vec，FIFO 队列
2. **访问检查**：使用 `(BasicBlock, Vec<BasicBlock>)` 作为 HashSet key
3. **状态管理**：在分支点保存和恢复 BindingManager 状态

## 参考资料

- [计划文档](../../.cursor/plans/k-predecessor_dfs_优化_*.plan.md)
- [并查集设计](STRING_ID_DESIGN.md)
- [Drop 追踪测试](src/toys/TEST_DROP_TRACKING.md)


# k-Predecessor DFS 配置指南

## 快速配置

### 1. 修改 k 值

打开 `src/callbacks.rs`，找到第 185-197 行：

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 2,        // 👈 修改这里
        max_visits_per_block: 10, // 👈 修改这里
    }
}
```

### 2. 启用统计信息（可选）

在同一文件的第 244 行，取消注释：

```rust
// 找到这行（约 244 行）
// print_dfs_stats(&name, &stats);

// 改为
print_dfs_stats(&name, &stats);
```

### 3. 运行

```bash
cargo run
```

## 配置参数说明

### k_predecessor（前序节点数量）

| 值 | 说明 | 性能 | 适用场景 |
|----|------|------|---------|
| 0 | 每个 block 只访问一次 | 最快 | 快速粗略分析 |
| 1 | 记录最近 1 个前序 | 较快 | 简单控制流 |
| 2 | 记录最近 2 个前序 | 中等 | **推荐**：日常分析 |
| 3 | 记录最近 3 个前序 | 较慢 | 复杂控制流 |
| 5+ | 记录最近 5+ 个前序 | 慢 | 高精度分析 |

### max_visits_per_block（最大访问次数）

防止无限循环，限制单个 block 的最大访问次数。

| 值 | 说明 |
|----|------|
| 10 | 默认值，适合大多数情况 |
| 20 | 允许更多重复访问，适合复杂循环 |
| 50+ | 接近完全路径敏感，性能开销大 |

## 推荐配置

### 场景 1：日常开发（推荐）

```rust
crate::dfs::DfsConfig {
    k_predecessor: 2,
    max_visits_per_block: 10,
}
```

### 场景 2：快速检查

```rust
crate::dfs::DfsConfig {
    k_predecessor: 0,
    max_visits_per_block: 10,
}
```

### 场景 3：关键代码精确分析

```rust
crate::dfs::DfsConfig {
    k_predecessor: 3,
    max_visits_per_block: 20,
}
```

## 高级配置

### 根据函数复杂度自动选择

修改 `get_dfs_config()` 为：

```rust
fn get_dfs_config_adaptive(body: &Body) -> crate::dfs::DfsConfig {
    let block_count = body.basic_blocks.len();
    
    let k = if block_count < 20 {
        3  // 小函数用高精度
    } else if block_count < 50 {
        2  // 中等函数
    } else {
        1  // 大函数用低精度
    };
    
    crate::dfs::DfsConfig {
        k_predecessor: k,
        max_visits_per_block: 10,
    }
}
```

然后在 `analyze_function()` 中改为：

```rust
let config = get_dfs_config_adaptive(body);
```

### 根据函数名选择配置

```rust
fn get_dfs_config_by_name(func_name: &str) -> crate::dfs::DfsConfig {
    if func_name.contains("critical") || func_name.contains("unsafe") {
        // 关键函数用高精度
        crate::dfs::DfsConfig {
            k_predecessor: 3,
            max_visits_per_block: 20,
        }
    } else {
        // 普通函数用默认配置
        crate::dfs::DfsConfig {
            k_predecessor: 2,
            max_visits_per_block: 10,
        }
    }
}
```

## 统计信息解读

启用统计后，会看到类似输出：

```
=== DFS Statistics for example::main ===
  Total visit attempts: 45        ← 总共尝试访问的次数
  Successful visits: 32           ← 成功访问的次数
  Skipped (duplicate path): 10   ← 因路径重复被跳过
  Skipped (max visits): 3        ← 因达到访问上限被跳过
  Unique paths explored: 32      ← 探索的唯一路径数
  Unique blocks visited: 15      ← 访问的唯一 block 数
  Path explosion factor: 2.13x   ← 路径爆炸因子
================================
```

### 路径爆炸因子

**计算公式**：`unique_paths / unique_blocks`

| 因子 | 说明 | 建议 |
|------|------|------|
| < 1.5x | 路径爆炸不明显 | 可以增大 k 值 |
| 1.5-3x | 中等路径爆炸 | 当前配置合理 |
| > 3x | 路径爆炸严重 | 减小 k 值或增大 max_visits |

## 性能影响

| k 值 | 预期时间开销 | 内存开销 |
|------|-------------|---------|
| 0 | 1.0x（基准） | 1.0x |
| 1 | 1.2-1.5x | 1.2x |
| 2 | 1.5-2.5x | 1.5x |
| 3 | 2-4x | 2.0x |
| 5+ | 3-10x | 3-5x |

## 故障排查

### 问题：分析时间过长

**原因**：k 值过大导致路径爆炸

**解决**：
1. 减小 k 值（如从 3 降到 2）
2. 减小 max_visits（如从 20 降到 10）
3. 启用统计信息查看路径爆炸因子

### 问题：检测不到某些 bug

**原因**：k 值过小，路径敏感性不足

**解决**：
1. 增大 k 值（如从 1 增到 3）
2. 增大 max_visits 允许更多重复访问

### 问题：内存占用过高

**原因**：路径数量过多

**解决**：
1. 减小 k 值
2. 减小 max_visits
3. 使用自适应配置（大函数用小 k）

## 总结

1. **默认配置**：`k=2, max_visits=10`（已设置）
2. **修改位置**：`src/callbacks.rs` 第 185-197 行
3. **启用统计**：取消注释第 244 行
4. **观察调整**：根据统计信息调整配置
5. **性能优先**：减小 k 值
6. **精度优先**：增大 k 值

配置完成后直接运行 `cargo run` 即可！


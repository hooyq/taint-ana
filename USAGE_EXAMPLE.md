# k-Predecessor DFS 使用示例

## 快速开始

配置参数直接在代码中设置，无需环境变量。

### 修改配置

打开 `src/callbacks.rs`，找到 `get_dfs_config()` 函数：

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 2,        // 修改这里的 k 值
        max_visits_per_block: 10, // 修改这里的访问上限
    }
}
```

### 运行分析

```bash
cd experiment/fn-signature-extractor/taintAna
cargo build
cargo run
```

## 不同配置示例

### 配置 1：快速模式（k=0）

**适用场景**：快速粗略分析，每个 block 只访问一次

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 0,
        max_visits_per_block: 10,
    }
}
```

### 配置 2：平衡模式（k=2）- 推荐

**适用场景**：平衡精度和性能，适合大多数情况

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 2,
        max_visits_per_block: 10,
    }
}
```

### 配置 3：高精度模式（k=3）

**适用场景**：复杂控制流分析

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 3,
        max_visits_per_block: 20,
    }
}
```

### 配置 4：完全路径敏感（k=100）

**适用场景**：小函数的精确分析

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 100,
        max_visits_per_block: 100,
    }
}
```

## 启用统计信息

在 `src/callbacks.rs` 的 `analyze_function()` 函数中，找到这行并取消注释：

```rust
// 找到这行
// print_dfs_stats(&name, &stats);

// 改为
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

## 测试不同的 k 值

### 步骤 1：修改配置

编辑 `src/callbacks.rs`，设置 k=0：

```rust
fn get_dfs_config() -> crate::dfs::DfsConfig {
    crate::dfs::DfsConfig {
        k_predecessor: 0,
        max_visits_per_block: 10,
    }
}
```

### 步骤 2：启用统计

取消注释统计输出：

```rust
print_dfs_stats(&name, &stats);
```

### 步骤 3：运行并记录结果

```bash
cargo run > results_k0.txt 2>&1
```

### 步骤 4：重复测试不同的 k 值

修改 k=1, 2, 3... 并分别运行，对比结果。

## 分析特定测试用例

### 分析 use_after_free 示例

```bash
cd toys/use_after_free
cargo build
```

### 分析自定义代码

创建测试文件 `src/test_example.rs`：

```rust
fn example_with_branches() {
    let x = vec![1, 2, 3];
    
    if some_condition() {
        let y = x;  // 分支 1：x 被 move
        drop(y);
    } else {
        let z = x;  // 分支 2：x 被 move
        drop(z);
    }
}

fn some_condition() -> bool {
    true
}
```

然后根据需要调整 k 值进行分析。

## 性能对比

### 小函数（< 20 blocks）

**推荐配置：**

```rust
crate::dfs::DfsConfig {
    k_predecessor: 2,
    max_visits_per_block: 10,
}
```

### 中等函数（20-50 blocks）

**推荐配置：**

```rust
crate::dfs::DfsConfig {
    k_predecessor: 1,
    max_visits_per_block: 15,
}
```

### 大函数（> 50 blocks）

**推荐配置：**

```rust
crate::dfs::DfsConfig {
    k_predecessor: 0,
    max_visits_per_block: 10,
}
```

## 调试技巧

### 1. 查看详细的路径信息

在 `src/callbacks.rs` 的访问器中添加调试输出：

```rust
dfs_visit_with_manager_ex(body, start, &mut manager, config, &mut |bb_idx, mgr, ctx| {
    // 添加这行查看路径信息
    println!("Block {:?}, predecessors: {:?}", bb_idx, ctx.predecessors);
    
    let bb = &body.basic_blocks[bb_idx];
    // ... 其余代码
});
```

### 2. 监控路径爆炸

启用统计信息并观察 `explosion factor`：

- **< 1.5x**：路径爆炸不明显，可以增大 k 值
- **1.5x - 3x**：中等路径爆炸，当前配置合理
- **> 3x**：路径爆炸严重，建议减小 k 值

### 3. 对比不同配置

创建一个测试脚本 `test_configs.sh`（Linux/Mac）：

```bash
#!/bin/bash

echo "Testing different k values..."

for k in 0 1 2 3; do
    echo "=== Testing k=$k ==="
    # 手动修改 src/callbacks.rs 中的 k 值
    # 或者使用 sed 自动替换
    sed -i "s/k_predecessor: [0-9]*/k_predecessor: $k/" src/callbacks.rs
    cargo run 2>&1 | grep "DFS Statistics" -A 10
    echo ""
done
```

或者 PowerShell 脚本 `test_configs.ps1`（Windows）：

```powershell
Write-Host "Testing different k values..."

foreach ($k in 0..3) {
    Write-Host "=== Testing k=$k ==="
    # 手动修改 src/callbacks.rs 中的 k 值
    (Get-Content src/callbacks.rs) -replace 'k_predecessor: \d+', "k_predecessor: $k" | Set-Content src/callbacks.rs
    cargo run 2>&1 | Select-String "DFS Statistics" -Context 0,10
    Write-Host ""
}
```

## 常见问题

### Q: 如何知道应该使用什么 k 值？

A: 从 k=1 开始，启用统计信息：

1. 修改配置为 k=1
2. 启用 `print_dfs_stats`
3. 运行并观察输出

观察指标：
- 如果 `skipped_duplicate_path` 很高 → 可以增大 k
- 如果 `explosion_factor > 3` → 应该减小 k
- 如果 `skipped_max_visits` > 0 → 可能需要增大 max_visits

### Q: 性能影响有多大？

A: 大致估算：
- k=0: 基准性能（1x）
- k=1: 1.2-1.5x 时间
- k=2: 1.5-2.5x 时间
- k=3: 2-4x 时间

具体取决于代码的控制流复杂度。

### Q: 如何针对不同函数使用不同配置？

A: 可以在 `get_dfs_config()` 中根据函数名动态选择：

```rust
fn get_dfs_config_for_function(func_name: &str) -> crate::dfs::DfsConfig {
    // 根据函数名选择配置
    if func_name.contains("critical") || func_name.contains("important") {
        // 关键函数使用高精度
        crate::dfs::DfsConfig {
            k_predecessor: 3,
            max_visits_per_block: 20,
        }
    } else {
        // 普通函数使用默认配置
        crate::dfs::DfsConfig {
            k_predecessor: 1,
            max_visits_per_block: 10,
        }
    }
}
```

然后在 `analyze_function()` 中调用：

```rust
let config = get_dfs_config_for_function(&name);
```

### Q: 如何根据函数复杂度自动选择 k 值？

A: 可以根据 BasicBlock 数量动态选择：

```rust
fn get_dfs_config_adaptive(body: &Body) -> crate::dfs::DfsConfig {
    let block_count = body.basic_blocks.len();
    
    let k = if block_count < 20 {
        3  // 小函数用高精度
    } else if block_count < 50 {
        2  // 中等函数用中等精度
    } else {
        1  // 大函数用低精度
    };
    
    crate::dfs::DfsConfig {
        k_predecessor: k,
        max_visits_per_block: 10,
    }
}
```

在 `analyze_function()` 中使用：

```rust
let config = get_dfs_config_adaptive(body);
```

## 高级用法

### 条件性启用统计信息

只对特定函数打印统计：

```rust
// 在 analyze_function() 中
if name.contains("example") || name.contains("test") {
    print_dfs_stats(&name, &stats);
}
```

### 收集所有函数的统计信息

创建一个全局统计收集器：

```rust
// 在文件顶部添加
use std::sync::Mutex;
use std::collections::HashMap;

lazy_static::lazy_static! {
    static ref GLOBAL_STATS: Mutex<HashMap<String, crate::dfs::DfsStats>> = 
        Mutex::new(HashMap::new());
}

// 在 analyze_function() 中
GLOBAL_STATS.lock().unwrap().insert(name.clone(), stats.clone());

// 在 analyze_crate() 结束时打印汇总
fn print_summary_stats() {
    let stats = GLOBAL_STATS.lock().unwrap();
    println!("\n=== Summary Statistics ===");
    for (func, stat) in stats.iter() {
        println!("{}: explosion_factor={:.2}x, paths={}", 
                 func, 
                 stat.unique_paths as f64 / stat.unique_blocks.max(1) as f64,
                 stat.unique_paths);
    }
}
```

## 配置建议总结

| 场景 | k 值 | max_visits | 说明 |
|------|------|------------|------|
| 快速粗略分析 | 0 | 10 | 最快，每个 block 只访问一次 |
| 日常开发分析 | 1-2 | 10-15 | **推荐**，平衡精度和性能 |
| 复杂控制流 | 2-3 | 15-20 | 较高精度，适度性能开销 |
| 关键函数精确分析 | 3-5 | 20-50 | 高精度，较大性能开销 |
| 小函数完全分析 | 10+ | 50-100 | 接近完全路径敏感 |

## 总结

1. **配置在代码中**：直接修改 `src/callbacks.rs` 中的 `get_dfs_config()` 函数
2. **默认使用 k=2**：适合大多数场景
3. **启用统计信息**：取消注释 `print_dfs_stats` 行
4. **根据需要调整**：观察统计信息，调整 k 值和 max_visits
5. **可以动态配置**：根据函数名或复杂度动态选择配置

所有配置都在代码中完成，无需设置环境变量！

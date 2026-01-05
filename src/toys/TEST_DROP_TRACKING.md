# Drop位置追踪集成测试

## 测试文件

`test_drop_tracking.rs` - 包含多个use after drop场景的测试文件

## 运行方式

### 使用cargo run分析单个文件

```bash
cd c:\Users\hyqho\Workspace\Rust\analysis\rustAnalyzer\experiment\fn-signature-extractor\taintAna
cargo run --bin taint-ana -- .\src\toys\test_drop_tracking.rs
```

### 使用cargo build分析整个项目

```bash
cd c:\Users\hyqho\Workspace\Rust\analysis\rustAnalyzer\experiment\fn-signature-extractor\taintAna
RUSTC_WRAPPER=taint-ana cargo build
```

## 预期输出

系统应该检测并报告以下信息：

1. **Use After Drop错误位置**
   - 哪个函数
   - 哪个变量
   - 使用发生在哪个BasicBlock和源码位置

2. **Drop发生位置** (新功能)
   - 被drop的变量
   - Drop类型（DropTerminator或DropFunctionCall）
   - Drop发生的BasicBlock和源码位置
   - Drop发生的函数名

3. **绑定关系**
   - 变量的绑定组根
   - 组内成员列表

## 测试场景

### 1. test_simple_use_after_drop
最简单的use after drop：
- 创建Box
- 显式drop
- 尝试使用

预期：应该报告drop在哪一行发生（drop(x)那一行）

### 2. test_move_then_drop
移动后drop：
- 创建Box x
- 移动到y
- drop y
- 尝试使用x

预期：应该报告是y被drop，且x和y在同一个绑定组

### 3. test_multiple_moves_then_drop
多次移动后drop：
- a -> b -> c -> d
- drop d
- 尝试使用a

预期：应该报告所有变量在同一组，d被drop的位置

### 4. test_drop_in_different_branch
分支中的drop：
- 在if分支中drop
- 分支外尝试使用

预期：应该追踪到在if分支中的drop位置

### 5. test_explicit_vs_implicit_drop
显式vs隐式drop：
- 显式调用std::mem::drop
- 隐式的作用域结束drop

预期：能区分DropFunctionCall和DropTerminator

### 6. test_partial_move
结构体字段的部分移动：
- 移动结构体的一个字段
- drop移动出的字段

预期：正确追踪字段级别的drop

## 验证要点

✅ Drop位置信息是否正确记录
✅ DropTerminatorKind类型是否正确（DropTerminator vs DropFunctionCall）
✅ 绑定组关系是否正确
✅ 多次移动的追踪是否准确
✅ 分支处理是否正确（每个分支独立状态）
✅ Drop信息在undrop时是否正确清除

## 调试

如果需要详细的调试输出，设置环境变量：

```bash
# Windows PowerShell
$env:DEBUG_MIR="1"
cargo run --bin taint-ana -- .\src\toys\test_drop_tracking.rs

# Windows CMD
set DEBUG_MIR=1
cargo run --bin taint-ana -- .\src\toys\test_drop_tracking.rs
```

这将显示每个drop操作的详细信息：
- drop前的状态
- drop后的状态
- 绑定组信息
- 路径压缩信息


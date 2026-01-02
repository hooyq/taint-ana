# Taint Analysis - Function Signature Extractor

这是一个基于 rustc_middle 的 Rust 项目分析工具，用于提取项目中所有函数的签名。

## 功能

- 提取 Rust 项目中所有函数的签名
- 显示函数名、参数类型、返回类型
- 标识 unsafe 和 async 函数

## 使用方法

### 1. 构建项目

```bash
cd taintAna
cargo build
```

### 2. 安装为 cargo 子命令

将编译后的二进制文件添加到 PATH，或者使用：

```bash
cargo install --path .
```

### 3. 分析项目

在要分析的项目目录中运行：

```bash
 cargo +nightly-2025-10-02 taint-ana
```

或者直接使用 rustc wrapper：

```bash
RUSTC_WRAPPER=taint-ana cargo build
```

## 输出示例

工具会输出所有函数的签名，格式如下：

```
=== Function Signatures for crate: my_crate ===
fn my_crate::main() -> ()
unsafe fn my_crate::unsafe_function(x: i32) -> i32
async fn my_crate::async_function() -> i32
=== Total: 3 functions ===
```

## 依赖

- Rust nightly 工具链（需要 rustc-dev 组件）
- 使用 rustc_middle 进行类型分析

## 注意事项

- 需要 nightly Rust 工具链
- 需要设置 RUST_SYSROOT 环境变量或使用 rustup


## 单文件 (debug用)

- cargo run --bin taint-ana -- path
- cargo run --bin taint-ana -- .\src\toys\example.rs
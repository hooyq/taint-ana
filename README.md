# Taint-ana -- A Flow Sensitive Rust Bug Detector

这是一个基于 rustc_middle 的 Rust 项目分析工具，用于分析Rust 项目并且找出其中的漏洞。

## 介绍
本项目是一个 flow sensitive的rust 分析工具

通过遍历mir basic block, 来模拟程序运行时，并且从中获取想要的信息来进一步分析

目前结合taint-analysis的思想，可以对double free, UAF等经典漏洞进行分析和挖掘

### 欢迎各界专业认识进行需求提供，帮助作者应用到更多真实场景下 谢谢！！！

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
#under the dir of the target project
 cargo +nightly-2025-10-02 taint-ana
```

或者直接使用 rustc wrapper：

```bash
RUSTC_WRAPPER=taint-ana cargo build
```

```bash
cd /home/hyq/workspace/rustExperiment/asterinas

# 设置 RUSTC_WRAPPER
export RUSTC_WRAPPER=/home/hyq/.cargo/bin/taint-ana

# 尝试构建
cargo osdk build
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


## sample
```
    cd src/toys/use_after_free
    cargo +nightly-2025-10-02 taint-ana
    
```

## Concept
can see the ppt to understand the concept
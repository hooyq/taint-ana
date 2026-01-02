//! Unified reporting module for taint analysis output.
//! Provides structured error reporting with MIR context.

use rustc_middle::mir::{Body, Statement, Terminator, BasicBlock, Local};
use rustc_index::Idx;
use log::{info, error};

use crate::state::BindingManager;

/// Check if info-level logging is enabled
fn is_info_enabled() -> bool {
    log::log_enabled!(log::Level::Info)
}

/// Check if debug-level logging is enabled
fn is_debug_enabled() -> bool {
    log::log_enabled!(log::Level::Debug)
}

/// Output function analysis start information
pub fn report_function_start(fn_name: &str, body: &Body) {
    if is_info_enabled() {
        println!("\n{}", "=".repeat(60));
        println!("🔍 分析函数: {}", fn_name);
        println!("   局部变量数: {}", body.local_decls.len());
        println!("   基本块数: {}", body.basic_blocks.len());
        println!("{}\n", "=".repeat(60));
    }
}

/// Output function analysis end
pub fn report_function_end(fn_name: &str) {
    if is_info_enabled() {
        println!("✅ 完成分析: {}\n", fn_name);
    }
}

/// Report use-after-drop error (Statement version)
pub fn report_use_after_drop_stmt(
    fn_name: &str,
    stmt: &Statement,
    bb: BasicBlock,
    local_id: &str,
    body: &Body,
    manager: &mut BindingManager,
) {
    println!("\n❌ 检测到错误: Use After Drop");
    println!("┌{}", "─".repeat(58));
    println!("│ 函数: {}", fn_name);
    println!("│ 变量: {}", local_id);
    println!("│ 位置: {:?}", stmt.source_info.span);
    println!("│ 基本块: {:?}", bb);
    println!("│");
    println!("│ MIR 语句:");
    println!("│   {:?}", stmt.kind);
    println!("│");
    
    // Print variable type information
    print_local_info(body, local_id);
    
    // Print binding group information
    print_drop_path(manager, local_id);
    
    // Display basic block context
    print_basic_block_context(body, bb);
    
    println!("└{}\n", "─".repeat(58));
    
    error!("Use after drop: {} in function {}", local_id, fn_name);
}

/// Report use-after-drop error (Terminator version)
pub fn report_use_after_drop_term(
    fn_name: &str,
    term: &Terminator,
    bb: BasicBlock,
    local_id: &str,
    body: &Body,
    manager: &mut BindingManager,
) {
    println!("\n❌ 检测到错误: Use After Drop");
    println!("┌{}", "─".repeat(58));
    println!("│ 函数: {}", fn_name);
    println!("│ 变量: {}", local_id);
    println!("│ 位置: {:?}", term.source_info.span);
    println!("│ 基本块: {:?}", bb);
    println!("│");
    println!("│ MIR Terminator:");
    println!("│   {:?}", term.kind);
    println!("│");
    
    // Print variable type information
    print_local_info(body, local_id);
    
    // Print binding group information
    print_drop_path(manager, local_id);
    
    // Display basic block context
    print_basic_block_context(body, bb);
    
    println!("└{}\n", "─".repeat(58));
    
    error!("Use after drop: {} in function {}", local_id, fn_name);
}

/// Display basic block context information
fn print_basic_block_context(body: &Body, bb: BasicBlock) {
    println!("│ 基本块上下文 [{:?}]:", bb);
    
    let block = &body.basic_blocks[bb];
    
    // Display last few statements (if any)
    let stmt_count = block.statements.len();
    let start = if stmt_count > 3 { stmt_count - 3 } else { 0 };
    
    for (idx, stmt) in block.statements.iter().enumerate().skip(start) {
        println!("│     [{}] {:?}", idx, stmt.kind);
    }
    
    // Display terminator
    if let Some(ref term) = block.terminator {
        println!("│     [T] {:?}", term.kind);
    }
}

/// Print variable definition information
fn print_local_info(body: &Body, local_id: &str) {
    if let Ok(local_idx) = local_id.trim_start_matches('_').parse::<usize>() {
        let local = Local::from_usize(local_idx);
        if let Some(local_decl) = body.local_decls.get(local) {
            println!("│ 变量类型: {:?}", local_decl.ty);
            println!("│ 可变性: {:?}", local_decl.mutability);
        }
    }
}

/// Display variable's drop path tracking
fn print_drop_path(manager: &mut BindingManager, local_id: &str) {
    println!("│");
    println!("│ 📊 变量状态追踪:");
    println!("│   当前状态: dropped={}", manager.is_dropped(local_id));
    
    if let Some((root_id, members)) = manager.find_group(local_id) {
        println!("│   绑定组根: {}", root_id);
        println!("│   组内成员: {:?}", members);
    }
}


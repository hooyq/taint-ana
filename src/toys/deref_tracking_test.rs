//! 解引用跟踪测试
//! 
//! 测试新的解引用 ID 跟踪功能，验证工具能够：
//! 1. 正确区分指针和解引用
//! 2. 检测 drop(*ptr) 后使用 *ptr 的错误
//! 3. 允许 drop(*ptr) 后使用 ptr（指针本身）
//! 4. 检测 drop(ptr) 后使用 *ptr 的错误（依赖检查）

use std::ptr;

/// 测试1：基本解引用跟踪
/// 
/// 期望：检测到 `drop(*ptr)` 后使用 `*ptr` 的错误
#[allow(unused)]
fn test_basic_deref() {
    let mut v = vec![1, 2, 3];
    let ptr = &mut v as *mut Vec<i32>;
    unsafe {
        drop(ptr::read(ptr));  // drop *ptr → 应该标记 "*ptr" 为 dropped
        let x = ptr::read(ptr); // ❌ 应该检测到：Use after drop: *ptr
    }
}

/// 测试2：指针本身 vs 解引用
/// 
/// 期望：允许 `drop(*ptr)` 后使用 `ptr`（指针本身）
#[allow(unused)]
fn test_pointer_vs_deref() {
    let mut v = vec![1, 2, 3];
    let ptr = &mut v as *mut Vec<i32>;
    unsafe {
        drop(ptr::read(ptr));  // drop *ptr
        let p2 = ptr;          // ✓ 使用指针本身，应该允许
    }
}

/// 测试3：依赖检查 - drop 指针后解引用
/// 
/// 期望：检测到 `drop(ptr)` 后使用 `*ptr` 的错误
/// 
/// 注意：这个测试比较特殊，因为 MIR 中通常不会直接 drop 原始指针。
/// 但对于引用转指针的情况，这个测试验证依赖检查是否工作。
#[allow(unused)]
fn test_deref_dependency() {
    let mut v = vec![1, 2, 3];
    let r = &mut v;
    let ptr = r as *mut Vec<i32>;
    
    // 在 MIR 中，drop(r) 会标记 r 为 dropped
    // 由于 ptr 绑定到 r，ptr 也会被标记为 dropped
    drop(r);
    
    unsafe {
        // 使用 *ptr 时，依赖检查会发现 ptr 已 dropped
        let x = ptr::read(ptr); // ❌ 应该报错：指针已 drop，无法解引用
    }
}

/// 测试4：静态变量不应误报
/// 
/// 期望：不误报，因为静态变量的指针在 drop 内容后仍然有效
#[allow(unused)]
fn test_static_deref_no_false_positive() {
    static mut GLOBAL: Option<Vec<i32>> = None;
    unsafe {
        GLOBAL = Some(vec![1, 2, 3]);
        let ptr = ptr::addr_of_mut!(GLOBAL);
        
        // drop 静态变量的内容（*ptr）
        drop(ptr::read(ptr));
        
        // 使用指针本身
        let p2 = ptr; // ✓ 应该允许
    }
}

/// 测试5：多层解引用
/// 
/// 期望：正确跟踪多层解引用
#[allow(unused)]
fn test_multi_deref() {
    let mut v = vec![1, 2, 3];
    let p1 = &mut v as *mut Vec<i32>;
    let p2 = &p1 as *const *mut Vec<i32>;
    
    unsafe {
        drop(ptr::read(*p2));  // drop **p2
        let x = ptr::read(*p2); // ❌ 应该检测到：Use after drop: **p2
    }
}

/// 测试6：字段后解引用
/// 
/// 期望：正确处理先访问字段再解引用的情况
#[allow(unused)]
fn test_field_then_deref() {
    struct Container {
        ptr: *mut i32,
    }
    
    let mut x = 42;
    let c = Container { ptr: &mut x };
    
    unsafe {
        // c.ptr 是字段访问，*c.ptr 是解引用
        drop(ptr::read(c.ptr));  // drop *c.ptr
        let y = ptr::read(c.ptr); // ❌ 应该检测到：Use after drop: *c.ptr
    }
}

/// 测试7：解引用后字段
/// 
/// 期望：正确处理先解引用再访问字段的情况
#[allow(unused)]
fn test_deref_then_field() {
    struct Data {
        value: i32,
    }
    
    let mut d = Data { value: 42 };
    let ptr = &mut d as *mut Data;
    
    unsafe {
        // (*ptr).value 是先解引用再字段访问
        // 在 MIR 中表示为 [Deref, Field(0)]
        let v1 = (*ptr).value;
        drop(ptr::read(ptr));  // drop *ptr
        let v2 = (*ptr).value; // ❌ 应该检测到：Use after drop: *ptr.value 或 *ptr
    }
}

fn main() {
    println!("=== Deref Tracking Tests ===");
    println!("These tests verify the new deref tracking functionality:");
    println!("1. ✓ Distinguish pointer vs dereference");
    println!("2. ✓ Detect use-after-drop for *ptr");
    println!("3. ✓ Allow using ptr after drop(*ptr)");
    println!("4. ✓ Detect use of *ptr after drop(ptr) (dependency check)");
    println!("5. ✓ Handle static variables correctly");
    println!("");
    
    // 注意：这些测试函数只是用于生成 MIR 供工具分析
    // 实际运行会导致未定义行为，所以不调用它们
    println!("Tests are for static analysis only, not for execution.");
    
    println!("\n=== Test completed ===");
}


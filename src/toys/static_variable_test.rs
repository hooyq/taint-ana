//! 测试静态变量的正确处理
//! 
//! 这个测试文件验证：
//! 1. Move 到静态变量不产生误报
//! 2. 局部变量的 use-after-free 仍然能被检测到

use std::ptr;

// ========== 测试1：静态变量赋值（不应该误报）==========

static mut HOST_NAME: Option<Vec<u8>> = None;
static mut HOST_ALIASES: Option<Vec<Vec<u8>>> = None;

#[allow(static_mut_refs)]
fn test_static_move_no_false_positive() {
    unsafe {
        // 这些赋值应该不产生误报
        // 因为 Vec 被 Move 到静态变量，有全局生命周期，永不 drop
        *ptr::addr_of_mut!(HOST_NAME) = Some(vec![b'a', b'b', b'c', 0]);
        *ptr::addr_of_mut!(HOST_ALIASES) = Some(vec![vec![0, 1, 2], vec![3, 4, 5]]);
        
        // 访问静态变量（合法）
        if let Some(ref name) = *ptr::addr_of!(HOST_NAME) {
            println!("Host name: {:?}", name);
        }
    }
}

// ========== 测试2：局部变量逃逸（应该检测到）==========

fn test_local_escape_should_detect() {
    let mut v = vec![1, 2, 3];
    let ptr = v.as_mut_ptr();
    
    // 显式 drop v
    drop(v);
    
    unsafe {
        // 这应该被检测到：使用已 drop 的 v 的指针
        println!("{}", *ptr);
    }
}

// ========== 测试3：静态变量引用（不应该误报）==========

static mut GLOBAL_BUFFER: [i32; 10] = [0; 10];

#[allow(static_mut_refs)]
fn test_static_ref_no_false_positive() {
    unsafe {
        // 获取静态变量的引用
        let buf_ref = ptr::addr_of_mut!(GLOBAL_BUFFER);
        
        // 修改静态变量
        (*buf_ref)[0] = 42;
        
        // 访问静态变量（合法）
        println!("Global buffer: {:?}", *buf_ref);
    }
}

// ========== 测试4：混合场景 ==========

static mut GLOBAL_PTR: Option<Vec<i32>> = None;

#[allow(static_mut_refs)]
fn test_mixed_scenario() {
    let local_vec = vec![10, 20, 30];
    
    unsafe {
        // Move 到静态变量（不应该误报）
        *ptr::addr_of_mut!(GLOBAL_PTR) = Some(local_vec);
        
        // 访问静态变量中的数据（合法）
        if let Some(ref v) = *ptr::addr_of!(GLOBAL_PTR) {
            println!("Global ptr points to: {:?}", v);
        }
    }
}

// ========== 测试5：局部变量重新赋值 ==========

fn test_local_reassignment() {
    let mut v = vec![1, 2, 3];
    let _ptr1 = v.as_mut_ptr();
    
    // 显式 drop
    drop(v);
    
    // 重新赋值（恢复状态）
    let mut v = vec![4, 5, 6];
    let ptr2 = v.as_mut_ptr();
    
    unsafe {
        // 这应该是合法的：使用新的 v 的指针
        println!("{}", *ptr2);
    }
}

// ========== 主函数 ==========

fn main() {
    println!("=== Test 1: Static Move (No False Positive) ===");
    test_static_move_no_false_positive();
    
    println!("\n=== Test 2: Local Escape (Should Detect) ===");
    test_local_escape_should_detect();
    
    println!("\n=== Test 3: Static Ref (No False Positive) ===");
    test_static_ref_no_false_positive();
    
    println!("\n=== Test 4: Mixed Scenario ===");
    test_mixed_scenario();
    
    println!("\n=== Test 5: Local Reassignment ===");
    test_local_reassignment();
    
    println!("\n=== All tests completed ===");
}


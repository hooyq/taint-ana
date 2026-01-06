//! 完整的 escape_to_global 测试
//! 
//! 这是用户提供的原始测试用例，用于验证静态变量误报是否被修复

use std::os::raw::{c_char, c_int};
use std::ptr;

#[repr(C)]
pub struct hostent {
    h_name: *mut c_char,
    h_aliases: *mut *mut c_char,
    h_addrtype: c_int,
    h_length: c_int,
    h_addr_list: *mut *mut c_char,
}

static mut HOST_ENTRY: hostent = hostent {
    h_name: ptr::null_mut(),
    h_aliases: ptr::null_mut(),
    h_addrtype: 0,
    h_length: 0,
    h_addr_list: ptr::null_mut(),
};

static mut HOST_NAME: Option<Vec<u8>> = None;
static mut HOST_ALIASES: Option<Vec<Vec<u8>>> = None;

#[allow(static_mut_refs)]
pub unsafe extern "C" fn gethostent() -> *const hostent {
    // 这两行之前产生误报，现在应该不再误报
    // 因为我们识别出这是 Move 到静态变量
    *ptr::addr_of_mut!(HOST_ALIASES) = Some(vec![vec![0, 1, 2], vec![3, 4, 5]]);
    *ptr::addr_of_mut!(HOST_NAME) = Some(vec![b'a', b'b', b'c', 0]);

    // raw pointer + Vec interior
    let aliases = (*ptr::addr_of_mut!(HOST_ALIASES)).as_mut().unwrap();
    let mut alias_ptrs: Vec<*mut c_char> = aliases
        .iter_mut()
        .map(|v| v.as_mut_ptr() as *mut c_char)
        .collect();
    alias_ptrs.push(ptr::null_mut());

    let entry = ptr::addr_of_mut!(HOST_ENTRY);
    (*entry).h_name = (*ptr::addr_of_mut!(HOST_NAME))
        .as_mut()
        .unwrap()
        .as_mut_ptr() as *mut c_char;
    
    // 真正的问题：alias_ptrs 是局部变量，会在函数结束时 drop
    // 但 h_aliases 指向它，导致悬垂指针
    // 注意：这个问题可能需要更高级的字段敏感分析才能检测
    (*entry).h_aliases = alias_ptrs.as_mut_ptr();
    
    (*entry).h_length = 4;

    entry as *const hostent
}

fn escape_to_global() {
    unsafe {
        let h = gethostent();
        // alias_ptrs 已 drop，h_aliases 悬垂
        // 这可能产生漏报（因为需要跨函数和字段敏感分析）
        println!("{:?}", *(*h).h_aliases);
    }
}

fn main() {
    println!("=== Escape to Global Test ===");
    println!("This test verifies:");
    println!("1. ✅ No false positives on static variable assignments");
    println!("2. ⚠️  May still miss the true bug (alias_ptrs escape)");
    println!("");
    
    escape_to_global();
    
    println!("\n=== Test completed ===");
}


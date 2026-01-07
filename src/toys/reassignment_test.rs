//! 重新赋值检测测试
//! 
//! 验证带有解引用和字段访问的重新赋值能够正确恢复 drop 状态

use std::path::PathBuf;

/// 测试1：字段 + 解引用的重新赋值
/// 
/// 期望：不误报
#[allow(unused)]
fn test_field_deref_reassign() {
    struct Container {
        inner: Option<Vec<i32>>,
    }
    
    let mut c = Container { inner: Some(vec![1, 2, 3]) };
    let ptr = &mut c as *mut Container;
    
    unsafe {
        // MIR: drop((*ptr).inner)
        drop((*ptr).inner.take());
        
        // MIR: (*ptr).inner = Some(...)
        // ✓ 应该恢复 (*ptr).inner 的状态
        (*ptr).inner = Some(vec![4, 5, 6]);
        
        // MIR: r = &mut (*ptr).inner
        let r = &mut (*ptr).inner;
        
        // ✓ 应该不报错
        if let Some(ref mut v) = r {
            v.push(7);
        }
    }
}

/// 测试2：模拟 callbacks.rs 的场景
/// 
/// 期望：不误报
#[allow(unused)]
fn test_pathbuf_reassign() {
    struct Config {
        path: PathBuf,
    }
    
    let mut config = Config {
        path: PathBuf::from("/some/path"),
    };
    let ptr = &mut config as *mut Config;
    
    unsafe {
        // 清空 path
        let _ = std::mem::take(&mut (*ptr).path);
        
        // 重新赋值
        (*ptr).path = PathBuf::from("/new/path");
        
        // 使用引用修改
        let r = &mut (*ptr).path;
        r.push("subdir");
        
        // ✓ 应该不报错
        println!("{:?}", r);
    }
}

/// 测试3：多层字段访问的重新赋值
/// 
/// 期望：不误报
#[allow(unused)]
fn test_nested_field_reassign() {
    struct Inner {
        value: i32,
    }
    
    struct Middle {
        inner: Inner,
    }
    
    struct Outer {
        middle: Middle,
    }
    
    let mut outer = Outer {
        middle: Middle {
            inner: Inner { value: 42 },
        },
    };
    let ptr = &mut outer as *mut Outer;
    
    unsafe {
        // drop 多层嵌套字段
        drop(std::mem::replace(&mut (*ptr).middle.inner, Inner { value: 0 }));
        
        // 重新赋值
        (*ptr).middle.inner = Inner { value: 100 };
        
        // 使用引用
        let r = &mut (*ptr).middle.inner;
        r.value += 1;
        
        // ✓ 应该不报错
        println!("{}", r.value);
    }
}

/// 测试4：解引用后重新赋值整个值
/// 
/// 期望：不误报
#[allow(unused)]
fn test_whole_deref_reassign() {
    let mut v = vec![1, 2, 3];
    let ptr = &mut v as *mut Vec<i32>;
    
    unsafe {
        // drop 整个解引用的值
        drop(std::mem::take(&mut *ptr));
        
        // 重新赋值整个值
        *ptr = vec![4, 5, 6];
        
        // 使用引用
        let r = &mut *ptr;
        r.push(7);
        
        // ✓ 应该不报错
        println!("{:?}", r);
    }
}

/// 测试5：复杂投影的重新赋值
/// 
/// 期望：不误报
#[allow(unused)]
fn test_complex_projection_reassign() {
    struct Data {
        items: Vec<Option<Vec<i32>>>,
    }
    
    let mut data = Data {
        items: vec![Some(vec![1, 2]), None, Some(vec![3, 4])],
    };
    let ptr = &mut data as *mut Data;
    
    unsafe {
        // drop 复杂投影的字段
        if let Some(inner) = (*ptr).items.get_mut(0).and_then(|x| x.take()) {
            drop(inner);
        }
        
        // 重新赋值
        if let Some(slot) = (*ptr).items.get_mut(0) {
            *slot = Some(vec![10, 20]);
        }
        
        // 使用引用
        if let Some(Some(ref mut v)) = (*ptr).items.get_mut(0) {
            v.push(30);
            // ✓ 应该不报错
            println!("{:?}", v);
        }
    }
}

fn main() {
    println!("=== Reassignment Detection Tests ===");
    println!("These tests verify that reassignment correctly restores drop state");
    println!("for places with derefs and field accesses.");
    println!("");
    println!("Expected: No false positives");
    println!("");
    
    // 注意：这些测试函数只是用于生成 MIR 供工具分析
    // 实际运行会导致未定义行为，所以不调用它们
    println!("Tests are for static analysis only, not for execution.");
    
    println!("\n=== Test completed ===");
}


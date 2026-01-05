//! 集成测试：验证Drop位置追踪功能
//! 
//! 这个文件包含多个测试场景，用于验证系统能够正确追踪和报告drop操作的位置

fn test_simple_use_after_drop() {
    let x = Box::new(42);
    drop(x);  // Drop发生在这里
    println!("{}", x);  // Use after drop - 应该报告drop在哪里发生
}

fn test_move_then_drop() {
    let x = Box::new(100);
    let y = x;  // x moved to y
    drop(y);    // Drop y (也就是drop了原来的x)
    println!("{}", x);  // Use after drop - 应该追踪到是y被drop了
}

fn test_multiple_moves_then_drop() {
    let a = Box::new(1);
    let b = a;
    let c = b;
    let d = c;
    drop(d);    // Drop d
    println!("{}", a);  // Use after drop - a, b, c, d都在同一个组，应该显示d被drop的位置
}

fn test_drop_in_different_branch() {
    let x = Box::new(5);
    let condition = true;
    
    if condition {
        drop(x);  // 在这个分支中drop
    }
    
    println!("{}", x);  // Use after drop - 应该显示在if分支中被drop
}

fn test_explicit_vs_implicit_drop() {
    // 显式drop
    {
        let x = Box::new(10);
        drop(x);  // 显式调用std::mem::drop
        // println!("{}", x);  // 这会报错
    }
    
    // 隐式drop
    {
        let y = Box::new(20);
        // y在作用域结束时自动drop
    }
}

fn test_partial_move() {
    struct Pair {
        a: Box<i32>,
        b: Box<i32>,
    }
    
    let pair = Pair {
        a: Box::new(1),
        b: Box::new(2),
    };
    
    let a_moved = pair.a;  // 部分移动
    drop(a_moved);         // Drop移动出来的字段
    
    // 注意：在真实的Rust中，pair现在处于部分moved状态
    // pair.a 不能再使用，但 pair.b 仍然可以使用
}

fn main() {
    // 这些函数故意包含use after drop错误
    // 用于测试drop位置追踪功能
    
    // 注释掉以避免实际编译错误
    // test_simple_use_after_drop();
    // test_move_then_drop();
    // test_multiple_moves_then_drop();
    // test_drop_in_different_branch();
    
    test_explicit_vs_implicit_drop();
    test_partial_move();
    
    println!("测试完成");
}


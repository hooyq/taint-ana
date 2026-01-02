fn example() {
    let ptr;
    // 创建一个 Vec（拥有堆内存）
    {
        let mut vec = vec![123];
        ptr = vec.as_mut_ptr();
    }

    println!("{}", unsafe { *ptr });


}
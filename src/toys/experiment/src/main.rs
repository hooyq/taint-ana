fn drop_in_match() {
    fn create_obj(i: i32) -> Option<Vec<i32>> {
        if i > 10 {
            Some(Vec::new())
        } else {
            None
        }
    }
    let ptr = match create_obj(11) {
        Some(mut v) => v.as_mut_ptr(),
        None => std::ptr::null_mut(),
    };
    unsafe {
        if !ptr.is_null() {
            println!("{}", *ptr);
        }
    }
}

fn main (){

}

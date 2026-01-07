fn escape_to_global() {
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

    pub unsafe extern "C" fn gethostent() -> *const hostent {
        *ptr::addr_of_mut!(HOST_ALIASES) = Some(vec![vec![0, 1, 2], vec![3, 4, 5]]);
        *ptr::addr_of_mut!(HOST_NAME) = Some(vec![b'a', b'b', b'c', 0]);

        // 🔥 raw pointer + Vec interior
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
        (*entry).h_aliases = alias_ptrs.as_mut_ptr(); // 🔥 points to stack Vec
        (*entry).h_length = 4;

        entry as *const hostent
    }

    unsafe {
        let h = gethostent();
        // alias_ptrs 已 drop，h_aliases 悬垂
        println!("{:?}", *(*h).h_aliases);
    }
}

fn main(){

}
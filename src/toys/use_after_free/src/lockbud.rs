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
fn escape_to_param() {
    use std::ptr;
    use std::sync::atomic::{AtomicPtr, Ordering};
    struct Owned<T> {
        data: T,
    }
    impl<T> Owned<T> {
        fn as_raw(&self) -> *mut T {
            &self.data as *const _ as *mut _
        }
    }
    fn opt_owned_as_raw<T>(val: &Option<Owned<T>>) -> *mut T {
        val.as_ref().map(Owned::as_raw).unwrap_or(ptr::null_mut())
    }
    struct Obj<T> {
        ptr: AtomicPtr<T>,
    }
    impl<T> Obj<T> {
        fn null() -> Self {
            Obj {
                ptr: AtomicPtr::new(ptr::null_mut()),
            }
        }
        fn load(&self, ord: Ordering) -> *mut T {
            self.ptr.load(ord)
        }
        fn store(&self, owned: Option<Owned<T>>, ord: Ordering) {
            self.ptr.store(opt_owned_as_raw(&owned), ord);
        }
    }
    let o = Obj::<Vec<i32>>::null();
    let owned = Some(Owned { data: Vec::new() });
    o.store(owned, Ordering::Relaxed);
    let p = o.load(Ordering::Relaxed);
    unsafe {
        println!("{:?}", *p);
    }
}

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


use libc::c_char;
use std::ffi::CStr;

unsafe fn fmt_time(date: &Date) -> *const c_char {
    let days = vec!["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = vec![
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let year = 1900 + date.tm_year;

    let time_str = format!(
        "{} {} {:2} {:02}:{:02}:{:02} {:4}\n\0\0\0\0\0\0\0\0\0\0\0\0\0",
        days[date.tm_wday as usize],
        months[date.tm_mon as usize],
        date.tm_mday,
        date.tm_hour,
        date.tm_min,
        date.tm_sec,
        year
    );
    time_str[0..26].as_ptr() as _
}

struct Date {
    tm_year: usize,
    tm_wday: usize,
    tm_mon: usize,
    tm_mday: usize,
    tm_hour: usize,
    tm_min: usize,
    tm_sec: usize,
}

fn escape_to_return() {
    let date = Date {
        tm_year: 1,
        tm_wday: 1,
        tm_mon: 1,
        tm_mday: 1,
        tm_hour: 1,
        tm_min: 1,
        tm_sec: 1,
    };
    unsafe {
        let ptr = fmt_time(&date);
        println!("{:?}", CStr::from_ptr(ptr));
    }
}




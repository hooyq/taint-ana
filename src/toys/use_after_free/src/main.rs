use std::mem;

mod example;
mod lockbud;
struct Buffer {
    data: Vec<u8>,
}

impl Buffer {
    // 构造一个指定长度的 buffer（内容初始化为 0）
    fn allocate(size: usize) -> Self {
        Buffer { data: vec![0u8; size] }
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    // 模拟拷贝数据到另一个 buffer
    fn copy_to(&self, dst: &mut Buffer) -> usize {
        let len = self.len().min(dst.len());
        dst.data[..len].copy_from_slice(&self.data[..len]);
        len
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

fn from(buffer: Buffer) -> Vec<u8> {
    let mut slice = Buffer::allocate(buffer.len());
    let len = buffer.copy_to(&mut slice);

    // ❗此处若不忘记 drop，会出现 double free，因为 Vec 会接管 slice.data 的内存
    //mem::forget(slice);

    unsafe {
        // 从裸指针构造 Vec，长度和容量必须对应
        Vec::from_raw_parts(slice.as_mut_ptr(), len, buffer.len())
    }
}

fn main() {
    let b = Buffer { data: vec![1, 2, 3, 4, 5] };
    let v = from(b);
    println!("{:?}", v);
}

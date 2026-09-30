//! Messages that end just before an inaccessible page. The x86 short and
//! medium paths read the last partial vector with masked loads, whose
//! masked-off bytes may lie in the next page; the load cannot fault, but the
//! CPU may take a slow path. Times `Key::hash` (and the streamed hasher) of
//! a message ending `gap` bytes before a `PROT_NONE` page, against the same
//! page offset with the next page readable and present. Linux x86-64 only.
//! Usage: pageend [len ...]
use std::hint::black_box;
use std::time::Instant;

unsafe extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
}

const PAGE: usize = 4096;

fn best(f: &mut dyn FnMut(), n: u64) -> f64 {
    let mut b = f64::INFINITY;
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        b = b.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
    }
    b
}

/// Two written pages; the second one then made inaccessible if `guard`.
fn pages(guard: bool) -> &'static mut [u8] {
    // PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS.
    let p = unsafe { mmap(core::ptr::null_mut(), 2 * PAGE, 3, 0x22, -1, 0) };
    assert!(!p.is_null() && p as isize != -1);
    // Write both pages, so that the second one is present when readable.
    let s = unsafe { core::slice::from_raw_parts_mut(p, 2 * PAGE) };
    for (i, b) in s.iter_mut().enumerate() {
        *b = (i * 7 + 1) as u8;
    }
    if guard {
        assert_eq!(unsafe { mprotect(p.add(PAGE), PAGE, 0) }, 0);
    }
    &mut s[..PAGE]
}

fn main() {
    let key = match std::env::var("RJ_BACKEND").as_deref() {
        Ok(name) => {
            let backend = match name {
                "sse" => raijuhash::Backend::X86Sse,
                "avx2" => raijuhash::Backend::X86Avx2,
                _ => raijuhash::Backend::X86Avx512,
            };
            let bytes: Vec<u8> = (0..raijuhash::KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
            raijuhash::Key::with_backend(bytes.as_slice().try_into().unwrap(), backend)
        },
        Err(_) => raijuhash::Key::from_seed(7),
    };
    let lens: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let (open, guarded) = (pages(false), pages(true));
    println!("{:>6} {:>5} {:>10} {:>10} {:>10} {:>10}", "bytes", "gap", "open ns", "guard ns", "open str", "guard str");
    for &len in &lens {
        for gap in [0usize, 1, 16, 63, 64, 127, 128] {
            let at = PAGE - gap - len;
            let (a, b) = (&open[at..at + len], &guarded[at..at + len]);
            assert_eq!(key.hash(a), key.hash(b));
            let n = 200_000;
            let t_open = best(&mut || { black_box(key.hash(black_box(a))); }, n);
            let t_guard = best(&mut || { black_box(key.hash(black_box(b))); }, n);
            let s_open = best(&mut || { let mut h = key.hasher(); h.update(black_box(a)); black_box(h.finalize()); }, n);
            let s_guard = best(&mut || { let mut h = key.hasher(); h.update(black_box(b)); black_box(h.finalize()); }, n);
            println!("{len:>6} {gap:>5} {t_open:>10.2} {t_guard:>10.2} {s_open:>10.2} {s_guard:>10.2}");
        }
    }
}

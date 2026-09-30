//! Multi-threaded and single-threaded RaijuHash through the public API.
use std::hint::black_box;
use std::time::Instant;
fn best(reps: usize, mut f: impl FnMut()) -> f64 {
    f();
    let mut b = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..reps {
            f();
        }
        b = b.min(t.elapsed().as_secs_f64() / reps as f64);
    }
    b
}
fn main() {
    let key = raijuhash::Key::from_seed(1);
    println!(
        "{:>8} {:>9} {:>9} {:>9} {:>9}   (GB/s)",
        "size", "raiju x1", "raiju x2", "raiju x4", "raiju x8"
    );
    for n in [1usize << 20, 2 << 20, 4 << 20, 8 << 20, 16 << 20, 64 << 20] {
        let buf = vec![7u8; n + 16384];
        let off = (4096usize.wrapping_sub(buf.as_ptr() as usize)) % 16384;
        let data = &buf[off..off + n];
        let reps = ((256 << 20) / n).max(2);
        let g = |s: f64| n as f64 / s / 1e9;
        let c1 = best(reps, || {
            black_box(key.hash(black_box(data)));
        });
        let c2 = best(reps, || {
            black_box(key.hash_parallel(black_box(data), 2));
        });
        let c4 = best(reps, || {
            black_box(key.hash_parallel(black_box(data), 4));
        });
        let c8 = best(reps, || {
            black_box(key.hash_parallel(black_box(data), 8));
        });
        println!(
            "{:>6}MiB {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
            n >> 20,
            g(c1),
            g(c2),
            g(c4),
            g(c8)
        );
    }
}

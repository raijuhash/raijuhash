//! Whole-buffer asm kernels with accumulators resident in registers.
#[cfg(target_arch = "aarch64")]
fn main() {
    use std::hint::black_box;
    use std::time::Instant;
    use experiments::asm_kernels::*;
    type K = unsafe fn(&mut [u128; 32], *const u8, *const u8, usize);
    let run = |name: &str, f: K, key_bytes: usize, total: usize, chunk: usize| {
        let mut data = vec![0u8; total + 8192];
        let mut key = vec![0u8; key_bytes + 8192];
        for (i, b) in data.iter_mut().chain(key.iter_mut()).enumerate() {
            *b = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29) as u8;
        }
        let d = unsafe { data.as_ptr().add(data.as_ptr().align_offset(4096)) };
        let k = unsafe { key.as_ptr().add(key.as_ptr().align_offset(4096)) };
        let mut acc = [0u128; 32];
        let pass = |acc: &mut [u128; 32]| unsafe { f(acc, black_box(d), black_box(k), total / chunk) };
        let t = Instant::now();
        let mut it = 0u64;
        while t.elapsed().as_secs_f64() < 0.1 {
            pass(&mut acc);
            it += 1;
        }
        let mut best = f64::INFINITY;
        for _ in 0..9 {
            let t = Instant::now();
            for _ in 0..it {
                pass(&mut acc);
            }
            best = best.min(t.elapsed().as_secs_f64() / it as f64);
        }
        black_box(acc);
        println!("{name:14} {total:9}  {:6.2} GB/s  {:5.2} cyc/128B", total as f64 / best / 1e9,
                 best * 3.19e9 / (total as f64 / 128.0));
    };
    for total in [65536usize, 1 << 20, 16 << 20] {
        run("chain64v2", asm_chain64v2, 65 * 128, total, 8192);
        run("chain64v2nt", asm_chain64v2nt, 65 * 128, total, 8192);
        run("chain64v2 loads", asm_chain64v2lo, 65 * 128, total, 8192);
        run("chain64v2 kf", asm_chain64v2kf, 65 * 128, total, 8192);
    }
}
#[cfg(not(target_arch = "aarch64"))]
fn main() {}

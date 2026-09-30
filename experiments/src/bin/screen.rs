//! Compression-only throughput screening of the candidate families on NEON.
//! Usage: screen [total_bytes]   (default 65536, hot in L1)

#[cfg(target_arch = "aarch64")]
mod run {
    use std::hint::black_box;
    use std::time::Instant;

    use experiments::neon::*;

    type Kernel = unsafe fn(&mut [V; 32], *const u8, *const u8);

    macro_rules! kernel {
        ($name:ident, |$acc:ident, $d:ident, $k:ident| $body:block) => {
            #[target_feature(enable = "neon,aes,sha3")]
            unsafe fn $name(out: &mut [V; 32], $d: *const u8, $k: *const u8) {
                let mut local = *out;
                let $acc = &mut local;
                unsafe { $body }
                *out = local;
            }
        };
    }

    // Each kernel processes one chunk; the chunk/key sizes are listed in `main`.
    macro_rules! stripe_kernel {
        ($name:ident, $mac:ident, $unit:literal, [$($j:literal)*], $n:literal, $stripes:literal) => {
            kernel!($name, |acc, d, k| {
                for s in 0..$stripes {
                    experiments::$mac!(acc, d.add(s * $unit * $n), k.add(s * $unit * ($n + 1)), [$($j)*], $n);
                }
            });
        };
    }

    stripe_kernel!(c4, chain_stripe, 128, [1 2 3], 4, 8);
    stripe_kernel!(c8, chain_stripe, 128, [1 2 3 4 5 6 7], 8, 4);
    stripe_kernel!(c12, chain_stripe, 128, [1 2 3 4 5 6 7 8 9 10 11], 12, 3);
    stripe_kernel!(c16, chain_stripe, 128, [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 2);
    stripe_kernel!(c16x4, chain_stripe, 128, [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 4);
    stripe_kernel!(c32b8, chain32_stripe, 32, [1 2 3 4 5 6 7], 8, 16);
    stripe_kernel!(c32b16, chain32_stripe, 32, [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 8);
    stripe_kernel!(u4, parity_stripe, 128, [0 1 2 3], 4, 8);
    stripe_kernel!(u8p, parity_stripe, 128, [0 1 2 3 4 5 6 7], 8, 4);
    stripe_kernel!(u16p, parity_stripe, 128, [0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 2);
    stripe_kernel!(c8cm, chain_stripe_cm, 128, [1 2 3 4 5 6 7], 8, 4);
    stripe_kernel!(c16cm, chain_stripe_cm, 128, [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 2);
    stripe_kernel!(u4cm, parity_stripe_cm, 128, [0 1 2 3], 4, 8);
    stripe_kernel!(u8cm, parity_stripe_cm, 128, [0 1 2 3 4 5 6 7], 8, 4);
    stripe_kernel!(u16cm, parity_stripe_cm, 128, [0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15], 16, 2);

    kernel!(nh, |acc, d, k| {
        for b in (0..32).step_by(2) {
            nh_block(&mut acc[0..], d.add(128 * b), k.add(128 * b));
            nh_block(&mut acc[4..], d.add(128 * b + 128), k.add(128 * b + 128));
        }
    });
    kernel!(d2, |acc, d, k| {
        for b in (0..32).step_by(2) {
            d2_block(&mut acc[0..], d.add(128 * b), k.add(256 * b));
            d2_block(&mut acc[8..], d.add(128 * b + 128), k.add(256 * b + 256));
        }
    });
    kernel!(l128, |acc, d, k| {
        for b in (0..32).step_by(2) {
            l128_block(&mut acc[0..], d.add(128 * b), k.add(256 * b));
            l128_block(&mut acc[6..], d.add(128 * b + 128), k.add(256 * b + 256));
        }
    });
    kernel!(i4, |acc, d, k| {
        for b in (0..32).step_by(2) {
            i4_block(&mut acc[0..], d.add(128 * b), k.add(512 * b));
            i4_block(&mut acc[4..], d.add(128 * b + 128), k.add(512 * b + 512));
        }
    });
    kernel!(m32, |acc, d, k| {
        for b in (0..32).step_by(2) {
            m32_block(&mut acc[0..], d.add(128 * b), k.add(128 * b));
            m32_block(&mut acc[4..], d.add(128 * b + 128), k.add(128 * b + 128));
        }
    });
    kernel!(poly, |acc, d, k| {
        let pw: &[V; 16] = &*(k as *const [V; 16]);
        let mut h = acc[0];
        for b in 0..32 {
            poly_block(&mut h, d.add(128 * b), pw);
        }
        acc[0] = h;
    });

    macro_rules! asm_kernel {
        ($name:ident, $f:path) => {
            unsafe fn $name(out: &mut [V; 32], d: *const u8, k: *const u8) {
                unsafe { $f(&mut *(out as *mut [V; 32] as *mut [u128; 32]), d, k) }
            }
        };
    }
    asm_kernel!(a_c8, experiments::asm_kernels::asm_chain8);
    asm_kernel!(a_c12, experiments::asm_kernels::asm_chain12);
    asm_kernel!(a_c16, experiments::asm_kernels::asm_chain16);
    asm_kernel!(a_u4, experiments::asm_kernels::asm_parity4);
    asm_kernel!(a_u8, experiments::asm_kernels::asm_parity8);
    asm_kernel!(a_u16, experiments::asm_kernels::asm_parity16);

    fn measure(name: &str, chunk: usize, key_bytes: usize, total: usize, f: Kernel) {
        measure_off(name, chunk, key_bytes, total, f, 0, 0);
    }

    fn measure_off(name: &str, chunk: usize, key_bytes: usize, total: usize, f: Kernel, doff: usize, koff: usize) {
        let total = total / chunk * chunk;
        let mut data = vec![0u8; total + 8192];
        let mut key = vec![0u8; key_bytes + 8192];
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for b in data.iter_mut().chain(key.iter_mut()) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        let d = unsafe { data.as_ptr().add(data.as_ptr().align_offset(4096) + doff) };
        let k = unsafe { key.as_ptr().add(key.as_ptr().align_offset(4096) + koff) };
        let pass = || {
            let mut acc = [zero(); 32];
            for c in 0..total / chunk {
                unsafe { f(&mut acc, black_box(d.add(c * chunk)), black_box(k)) };
            }
            black_box(acc);
        };
        let start = Instant::now();
        let mut iters = 0u64;
        while start.elapsed().as_secs_f64() < 0.05 {
            pass();
            iters += 1;
        }
        let iters = iters.max(1);
        let mut best = f64::INFINITY;
        for _ in 0..7 {
            let t = Instant::now();
            for _ in 0..iters {
                pass();
            }
            best = best.min(t.elapsed().as_secs_f64() / iters as f64);
        }
        let gbs = total as f64 / best / 1e9;
        let cyc = best * 3.182e9 / (total as f64 / 128.0);
        println!("{name:10} chunk {chunk:5} key {key_bytes:5} doff {doff:4} koff {koff:4}  {gbs:7.2} GB/s  {cyc:5.2} cyc/128B");
    }

    pub fn main() {
        let total: usize = std::env::args().nth(1).map_or(65536, |s| s.parse().unwrap());
        println!("total {total} bytes per pass");
        if std::env::args().nth(2).as_deref() == Some("offsets") {
            for koff in [0usize, 16, 32, 64, 128, 256, 512, 1024, 2048, 3072] {
                measure_off("asm c16", 4096, 2 * 17 * 128, total, a_c16, 0, koff);
            }
            for koff in [0usize, 64, 1024, 2048] {
                measure_off("nh", 4096, 4096, total, nh, 0, koff);
            }
            return;
        }
        measure("nh(2^-64)", 4096, 4096, total, nh);
        measure("c4", 4096, 8 * 5 * 128, total, c4);
        measure("c8", 4096, 4 * 9 * 128, total, c8);
        measure("c12", 4608, 3 * 13 * 128, total, c12);
        measure("c16", 4096, 2 * 17 * 128, total, c16);
        measure("c32b8", 4096, 16 * 9 * 32, total, c32b8);
        measure("c32b16", 4096, 8 * 17 * 32, total, c32b16);
        measure("u4", 4096, 8 * 5 * 128, total, u4);
        measure("u8", 4096, 4 * 9 * 128, total, u8p);
        measure("u16", 4096, 2 * 17 * 128, total, u16p);
        measure("asm c8", 4096, 4 * 9 * 128, total, a_c8);
        measure("asm c12", 4608, 3 * 13 * 128, total, a_c12);
        measure("asm c16", 4096, 2 * 17 * 128, total, a_c16);
        measure("asm u4", 4096, 8 * 5 * 128, total, a_u4);
        measure("asm u8", 4096, 4 * 9 * 128, total, a_u8);
        measure("asm u16", 4096, 2 * 17 * 128, total, a_u16);
        measure("c8cm", 4096, 4 * 9 * 128, total, c8cm);
        measure("c16cm", 4096, 2 * 17 * 128, total, c16cm);
        measure("u4cm", 4096, 8 * 5 * 128, total, u4cm);
        measure("u8cm", 4096, 4 * 9 * 128, total, u8cm);
        measure("u16cm", 4096, 2 * 17 * 128, total, u16cm);
        measure("d2", 4096, 8192, total, d2);
        measure("l128", 4096, 8192, total, l128);
        measure("i4", 4096, 16384, total, i4);
        measure("m32", 4096, 4096, total, m32);
        measure("poly", 4096, 256, total, poly);
    }
}

fn main() {
    #[cfg(target_arch = "aarch64")]
    run::main();
}

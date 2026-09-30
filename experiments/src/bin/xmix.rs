//! Bare mixed kernels on L1-resident data: v1 carryless blocks and
//! transposed Multimixer tiles in fixed ratios, no chunk close. Reports
//! bytes per cycle at the nominal 4.49 GHz (Zen 5 c8a boost clock).

#[cfg(target_arch = "x86_64")]
mod run {
    use std::hint::black_box;
    use std::time::Instant;

    use experiments::x86::*;

    const GHZ: f64 = 4.49;

    /// `NC` carryless blocks then `NT` tiles, repeated over `reps` groups of
    /// data (the same hot 16 KiB region), keys likewise.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    unsafe fn mix<const NC: usize, const NT: usize>(d: *const u8, k: *const u8, reps: usize) -> [Z; 8] {
        unsafe {
            let mut ch = Chain::new();
            let mut z = [zero(); 8];
            let group = 128 * NC + 512 * NT;
            let per = (8192 / group).max(1);
            for r in 0..reps {
                let base = (r % per) * group;
                let (d, k) = (d.add(base), opaque(k).add(base));
                for v in 0..NC {
                    ch.block(v & 7, d.add(128 * v), k.add(128 * v));
                }
                for t in 0..NT {
                    mm_tile(&mut z, d.add(128 * NC + 512 * t), k.add(128 * NC + 512 * t));
                }
            }
            let c = xor(ch.close(k), ch.digest_all());
            [c, z[0], z[1], z[2], z[3], xor(z[4], z[7]), z[5], z[6]]
        }
    }

    /// `NC` carryless blocks per tile, the tile's five pieces spread evenly
    /// between the blocks.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    unsafe fn inter<const NC: usize>(d: *const u8, k: *const u8, reps: usize) -> [Z; 8] {
        unsafe {
            let mut ch = Chain::new();
            let mut z = [zero(); 8];
            let group = 128 * NC + 512;
            let per = (8192 / group).max(1);
            for r in 0..reps {
                let base = (r % per) * group;
                let (d, k) = (d.add(base), opaque(k).add(base));
                let tp = TileParts::load(d.add(128 * NC), k.add(128 * NC));
                let mut piece = 0;
                for v in 0..NC {
                    ch.block(v & 7, d.add(128 * v), k.add(128 * v));
                    // After block v, pieces up to (v + 1) * 5 / NC.
                    while piece < ((v + 1) * 5) / NC {
                        if piece == 0 {
                            // `load` was issued up front; nothing to add.
                        } else {
                            tp.prod(&mut z, piece - 1);
                        }
                        piece += 1;
                    }
                }
                while piece < 5 {
                    if piece > 0 {
                        tp.prod(&mut z, piece - 1);
                    }
                    piece += 1;
                }
            }
            let c = xor(ch.close(k), ch.digest_all());
            [c, z[0], z[1], z[2], z[3], xor(z[4], z[7]), z[5], z[6]]
        }
    }

    pub fn main() {
        let random = std::env::args().nth(1).as_deref() == Some("random");
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        let mut fill = |c: u8| -> u8 {
            if !random {
                return c;
            }
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            st as u8
        };
        let buf: Vec<u8> = (0..16384 + 4096).map(|_| fill(3)).collect();
        let kb: Vec<u8> = (0..16384 + 4096).map(|_| fill(5)).collect();
        let d = unsafe { buf.as_ptr().add((4096 - buf.as_ptr() as usize % 4096) % 4096) };
        let k = unsafe { kb.as_ptr().add((4096 - kb.as_ptr() as usize % 4096) % 4096) };
        macro_rules! case {
            ($nc:literal, $nt:literal) => {{
                let bytes_per = 128 * $nc + 512 * $nt;
                let reps = (1 << 24) / bytes_per;
                let mut best = f64::INFINITY;
                for _ in 0..7 {
                    let t = Instant::now();
                    black_box(unsafe { mix::<$nc, $nt>(black_box(d), black_box(k), reps) });
                    best = best.min(t.elapsed().as_secs_f64());
                }
                let bytes = (reps * bytes_per) as f64;
                println!("{:>3} cl blocks + {:>2} MM tiles ({:>4} B cl : {:>4} B int): {:6.1} GB/s  {:5.1} B/cycle",
                    $nc, $nt, 128 * $nc, 512 * $nt, bytes / best / 1e9, bytes / best / 1e9 / GHZ);
            }};
        }
        case!(8, 0);
        case!(0, 1);
        case!(0, 2);
        case!(2, 1);
        case!(4, 1);
        case!(6, 1);
        case!(8, 1);
        case!(12, 1);
        case!(16, 1);
        case!(8, 2);
        case!(16, 2);
        case!(16, 3);
        macro_rules! icase {
            ($nc:literal) => {{
                let bytes_per = 128 * $nc + 512;
                let reps = (1 << 24) / bytes_per;
                let mut best = f64::INFINITY;
                for _ in 0..7 {
                    let t = Instant::now();
                    black_box(unsafe { inter::<$nc>(black_box(d), black_box(k), reps) });
                    best = best.min(t.elapsed().as_secs_f64());
                }
                let bytes = (reps * bytes_per) as f64;
                println!("interleaved {:>3} cl blocks per MM tile: {:6.1} GB/s  {:5.1} B/cycle", $nc, bytes / best / 1e9, bytes / best / 1e9 / GHZ);
            }};
        }
        icase!(4);
        icase!(6);
        icase!(8);
        icase!(10);
        icase!(12);
        icase!(14);
        icase!(16);
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}

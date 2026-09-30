//! Streaming-address version of the NH/chain inner loops written in asm, to
//! separate memory-system effects from compiler scheduling.

#[cfg(target_arch = "aarch64")]
mod run {
    use core::arch::asm;
    use std::hint::black_box;
    use std::time::Instant;

    fn bench(name: &str, total: usize, f: &dyn Fn(*const u8, *const u8, usize)) {
        let mut data = vec![1u8; total + 128];
        let mut key = vec![2u8; 16384 + 128];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i * 7) as u8;
        }
        for (i, b) in key.iter_mut().enumerate() {
            *b = (i * 13) as u8;
        }
        let d = unsafe { data.as_ptr().add(data.as_ptr().align_offset(128)) };
        let k = unsafe { key.as_ptr().add(key.as_ptr().align_offset(128)) };
        let start = Instant::now();
        let mut it = 0;
        while start.elapsed().as_secs_f64() < 0.05 {
            f(d, k, total);
            it += 1;
        }
        let mut best = f64::INFINITY;
        for _ in 0..7 {
            let t = Instant::now();
            for _ in 0..it {
                f(black_box(d), black_box(k), total);
            }
            best = best.min(t.elapsed().as_secs_f64() / it as f64);
        }
        println!(
            "{name:40} {:6.2} GB/s {:5.2} cyc/128B",
            total as f64 / best / 1e9,
            best * 3.19e9 / (total as f64 / 128.0)
        );
    }

    /// Plain NH over `total` bytes, key table of 4 KiB reused per chunk.
    fn nh(d: *const u8, k: *const u8, total: usize) {
        unsafe {
            asm!(
                "movi v0.16b, #0", "movi v1.16b, #0", "movi v2.16b, #0", "movi v3.16b, #0",
                "3:",
                "mov {kp}, {k}",
                "mov {cnt}, #32",
                "2:",
                "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                "ldp q24, q25, [{kp}]", "ldp q26, q27, [{kp}, #32]",
                "ldp q28, q29, [{kp}, #64]", "ldp q30, q31, [{kp}, #96]",
                "add {dp}, {dp}, #128", "add {kp}, {kp}, #128",
                "eor v16.16b, v16.16b, v24.16b", "eor v20.16b, v20.16b, v28.16b",
                "pmull v24.1q, v16.1d, v20.1d", "pmull2 v28.1q, v16.2d, v20.2d",
                "eor3 v0.16b, v0.16b, v24.16b, v28.16b",
                "eor v17.16b, v17.16b, v25.16b", "eor v21.16b, v21.16b, v29.16b",
                "pmull v25.1q, v17.1d, v21.1d", "pmull2 v29.1q, v17.2d, v21.2d",
                "eor3 v1.16b, v1.16b, v25.16b, v29.16b",
                "eor v18.16b, v18.16b, v26.16b", "eor v22.16b, v22.16b, v30.16b",
                "pmull v26.1q, v18.1d, v22.1d", "pmull2 v30.1q, v18.2d, v22.2d",
                "eor3 v2.16b, v2.16b, v26.16b, v30.16b",
                "eor v19.16b, v19.16b, v27.16b", "eor v23.16b, v23.16b, v31.16b",
                "pmull v27.1q, v19.1d, v23.1d", "pmull2 v31.1q, v19.2d, v23.2d",
                "eor3 v3.16b, v3.16b, v27.16b, v31.16b",
                "subs {cnt}, {cnt}, #1", "b.ne 2b",
                "subs {chunks}, {chunks}, #1", "b.ne 3b",
                dp = inout(reg) d => _, kp = out(reg) _, k = in(reg) k, cnt = out(reg) _,
                chunks = inout(reg) total / 4096 => _,
                out("v0") _, out("v1") _, out("v2") _, out("v3") _,
                out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
                out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
            );
        }
    }

    /// Same loads as NH but no arithmetic.
    fn loads_only(d: *const u8, k: *const u8, total: usize) {
        unsafe {
            asm!(
                "3:",
                "mov {kp}, {k}",
                "mov {cnt}, #32",
                "2:",
                "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                "ldp q24, q25, [{kp}]", "ldp q26, q27, [{kp}, #32]",
                "ldp q28, q29, [{kp}, #64]", "ldp q30, q31, [{kp}, #96]",
                "add {dp}, {dp}, #128", "add {kp}, {kp}, #128",
                "subs {cnt}, {cnt}, #1", "b.ne 2b",
                "subs {chunks}, {chunks}, #1", "b.ne 3b",
                dp = inout(reg) d => _, kp = out(reg) _, k = in(reg) k, cnt = out(reg) _,
                chunks = inout(reg) total / 4096 => _,
                out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
                out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
            );
        }
    }

    /// Data loads only (no key traffic).
    fn data_only(d: *const u8, _k: *const u8, total: usize) {
        unsafe {
            asm!(
                "2:",
                "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                "add {dp}, {dp}, #128",
                "subs {cnt}, {cnt}, #1", "b.ne 2b",
                dp = inout(reg) d => _, cnt = inout(reg) total / 128 => _,
                out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                out("v21") _, out("v22") _, out("v23") _,
            );
        }
    }

    macro_rules! loads_ratio {
        ($name:ident, $keyloads:literal, $pf:literal) => {
            fn $name(d: *const u8, k: *const u8, total: usize) {
                unsafe {
                    asm!(
                        "3:",
                        "mov {kp}, {k}",
                        "mov {cnt}, #32",
                        "2:",
                        $pf,
                        "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                        "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                        ".if {kl} >= 2\n ldp q24, q25, [{kp}]\n .endif",
                        ".if {kl} >= 4\n ldp q26, q27, [{kp}, #32]\n .endif",
                        ".if {kl} >= 6\n ldp q28, q29, [{kp}, #64]\n .endif",
                        ".if {kl} >= 8\n ldp q30, q31, [{kp}, #96]\n .endif",
                        "add {dp}, {dp}, #128", "add {kp}, {kp}, #128",
                        "subs {cnt}, {cnt}, #1", "b.ne 2b",
                        "subs {chunks}, {chunks}, #1", "b.ne 3b",
                        dp = inout(reg) d => _, kp = out(reg) _, k = in(reg) k, cnt = out(reg) _,
                        chunks = inout(reg) total / 4096 => _, kl = const $keyloads,
                        out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                        out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
                        out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
                    );
                }
            }
        };
    }
    loads_ratio!(r0, 0, "");
    loads_ratio!(r25, 2, "");
    loads_ratio!(r50, 4, "");
    loads_ratio!(r75, 6, "");
    loads_ratio!(r100, 8, "");
    loads_ratio!(r100pf256, 8, "prfm pldl1keep, [{dp}, #256]");
    loads_ratio!(r100pf512, 8, "prfm pldl1keep, [{dp}, #512]");
    loads_ratio!(r100pf1k, 8, "prfm pldl1keep, [{dp}, #1024]");
    loads_ratio!(r100pf2k, 8, "prfm pldl1strm, [{dp}, #2048]");
    loads_ratio!(r50pf1k, 4, "prfm pldl1keep, [{dp}, #1024]");

    macro_rules! loads_var {
        ($name:ident, $body:literal, $step:literal) => {
            fn $name(d: *const u8, k: *const u8, total: usize) {
                unsafe {
                    asm!(
                        "3:",
                        "mov {kp}, {k}",
                        "mov {cnt}, #(4096 / {step})",
                        "2:",
                        $body,
                        "add {dp}, {dp}, #{step}", "add {kp}, {kp}, #{step}",
                        "subs {cnt}, {cnt}, #1", "b.ne 2b",
                        "subs {chunks}, {chunks}, #1", "b.ne 3b",
                        dp = inout(reg) d => _, kp = out(reg) _, k = in(reg) k, cnt = out(reg) _,
                        chunks = inout(reg) total / 4096 => _, step = const $step, out("x9") _, out("x10") _,
                        out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                        out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
                        out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
                    );
                }
            }
        };
    }
    loads_var!(nt_data, "ldnp q16, q17, [{dp}]\n ldnp q18, q19, [{dp}, #32]\n ldnp q20, q21, [{dp}, #64]\n ldnp q22, q23, [{dp}, #96]\n ldp q24, q25, [{kp}]\n ldp q26, q27, [{kp}, #32]\n ldp q28, q29, [{kp}, #64]\n ldp q30, q31, [{kp}, #96]", 128);
    loads_var!(key_first, "ldp q24, q25, [{kp}]\n ldp q26, q27, [{kp}, #32]\n ldp q28, q29, [{kp}, #64]\n ldp q30, q31, [{kp}, #96]\n ldp q16, q17, [{dp}]\n ldp q18, q19, [{dp}, #32]\n ldp q20, q21, [{dp}, #64]\n ldp q22, q23, [{dp}, #96]", 128);
    loads_var!(interleave, "ldp q16, q17, [{dp}]\n ldp q24, q25, [{kp}]\n ldp q18, q19, [{dp}, #32]\n ldp q26, q27, [{kp}, #32]\n ldp q20, q21, [{dp}, #64]\n ldp q28, q29, [{kp}, #64]\n ldp q22, q23, [{dp}, #96]\n ldp q30, q31, [{kp}, #96]", 128);
    loads_var!(data_ahead, "ldp q16, q17, [{dp}, #0]\n ldp q18, q19, [{dp}, #32]\n ldp q20, q21, [{dp}, #64]\n ldp q22, q23, [{dp}, #96]\n ldp q16, q17, [{dp}, #128]\n ldp q18, q19, [{dp}, #160]\n ldp q20, q21, [{dp}, #192]\n ldp q22, q23, [{dp}, #224]\n ldp q24, q25, [{kp}]\n ldp q26, q27, [{kp}, #32]\n ldp q28, q29, [{kp}, #64]\n ldp q30, q31, [{kp}, #96]\n ldp q24, q25, [{kp}, #128]\n ldp q26, q27, [{kp}, #160]\n ldp q28, q29, [{kp}, #192]\n ldp q30, q31, [{kp}, #224]", 256);
    loads_var!(ld1x4, "ld1 {{v16.16b-v19.16b}}, [{dp}]\n add x9, {dp}, #64\n ld1 {{v20.16b-v23.16b}}, [x9]\n ld1 {{v24.16b-v27.16b}}, [{kp}]\n add x10, {kp}, #64\n ld1 {{v28.16b-v31.16b}}, [x10]", 128);

    /// Two data streams (first and second half) interleaved, keys r=1.
    fn two_streams(d: *const u8, k: *const u8, total: usize) {
        unsafe {
            let d2 = d.add(total / 2);
            asm!(
                "3:",
                "mov {kp}, {k}",
                "mov {cnt}, #32",
                "2:",
                "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                "ldp q24, q25, [{kp}]", "ldp q26, q27, [{kp}, #32]",
                "ldp q28, q29, [{kp}, #64]", "ldp q30, q31, [{kp}, #96]",
                "ldp q16, q17, [{dq}]", "ldp q18, q19, [{dq}, #32]",
                "ldp q20, q21, [{dq}, #64]", "ldp q22, q23, [{dq}, #96]",
                "ldp q24, q25, [{kp}]", "ldp q26, q27, [{kp}, #32]",
                "ldp q28, q29, [{kp}, #64]", "ldp q30, q31, [{kp}, #96]",
                "add {dp}, {dp}, #128", "add {dq}, {dq}, #128", "add {kp}, {kp}, #128",
                "subs {cnt}, {cnt}, #1", "b.ne 2b",
                "subs {chunks}, {chunks}, #1", "b.ne 3b",
                dp = inout(reg) d => _, dq = inout(reg) d2 => _, kp = out(reg) _, k = in(reg) k,
                cnt = out(reg) _, chunks = inout(reg) total / 8192 => _,
                out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
                out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
            );
        }
    }
    /// Two data streams, no keys.
    fn two_streams_data(d: *const u8, _k: *const u8, total: usize) {
        unsafe {
            let d2 = d.add(total / 2);
            asm!(
                "2:",
                "ldp q16, q17, [{dp}]", "ldp q18, q19, [{dp}, #32]",
                "ldp q20, q21, [{dp}, #64]", "ldp q22, q23, [{dp}, #96]",
                "ldp q16, q17, [{dq}]", "ldp q18, q19, [{dq}, #32]",
                "ldp q20, q21, [{dq}, #64]", "ldp q22, q23, [{dq}, #96]",
                "add {dp}, {dp}, #128", "add {dq}, {dq}, #128",
                "subs {cnt}, {cnt}, #1", "b.ne 2b",
                dp = inout(reg) d => _, dq = inout(reg) d2 => _, cnt = inout(reg) total / 256 => _,
                out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
                out("v21") _, out("v22") _, out("v23") _,
            );
        }
    }

    pub fn main() {
        for total in [65536usize, 262144, 1 << 20, 16 << 20] {
            println!("-- {total} bytes");
            bench("data loads only", total, &data_only);
            bench("r=0", total, &r0);
            bench("r=0.25", total, &r25);
            bench("r=0.5", total, &r50);
            bench("r=0.75", total, &r75);
            bench("r=1", total, &r100);
            bench("r=1 prefetch 256", total, &r100pf256);
            bench("r=1 prefetch 512", total, &r100pf512);
            bench("r=1 prefetch 1k", total, &r100pf1k);
            bench("r=1 prefetch strm 2k", total, &r100pf2k);
            bench("r=0.5 prefetch 1k", total, &r50pf1k);
            bench("nh asm", total, &nh);
            bench("r=1 ldnp data", total, &nt_data);
            bench("r=1 key first", total, &key_first);
            bench("r=1 interleaved", total, &interleave);
            bench("r=1 2 blocks grouped", total, &data_ahead);
            bench("r=1 ld1x4", total, &ld1x4);
            bench("two streams data only", total, &two_streams_data);
            bench("two streams r=1", total, &two_streams);
        }
        let _ = loads_only;
    }
}

fn main() {
    #[cfg(target_arch = "aarch64")]
    run::main();
}

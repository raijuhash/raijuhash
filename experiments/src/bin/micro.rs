//! Instruction throughput probes for the local AArch64 core.
//! Reports cycles per instruction using a dependent-add loop as the clock.

#[cfg(target_arch = "aarch64")]
mod probes {
    use core::arch::asm;
    use std::time::Instant;

    const ITERS: u64 = 20_000_000;

    fn time<F: Fn()>(f: F) -> f64 {
        f();
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let t = Instant::now();
            f();
            best = best.min(t.elapsed().as_secs_f64());
        }
        best
    }

    /// Nanoseconds per cycle, from a chain of dependent 1-cycle adds.
    pub fn ns_per_cycle() -> f64 {
        let s = time(|| unsafe {
            let mut x: u64 = 0;
            asm!(
                "2:",
                ".rept 100", "add {x}, {x}, #1", ".endr",
                "subs {n}, {n}, #1", "b.ne 2b",
                x = inout(reg) x, n = inout(reg) ITERS / 10 => _,
            );
            std::hint::black_box(x);
        });
        s * 1e9 / (ITERS as f64 / 10.0 * 100.0)
    }

    macro_rules! probe {
        ($name:expr, $per:expr, $body:expr) => {{
            let s = time(|| unsafe {
                let buf = [0u8; 8192];
                let p = buf.as_ptr();
                asm!(
                    "movi v0.16b, #1", "movi v1.16b, #2", "movi v2.16b, #3", "movi v3.16b, #4",
                    "movi v4.16b, #5", "movi v5.16b, #6", "movi v6.16b, #7", "movi v7.16b, #8",
                    "/* {p} */",
                    "2:",
                    $body,
                    "subs {n}, {n}, #1", "b.ne 2b",
                    n = inout(reg) ITERS / 10 => _,
                    p = in(reg) p,
                    out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
                    out("v6") _, out("v7") _, out("v16") _, out("v17") _, out("v18") _,
                    out("v19") _, out("v20") _, out("v21") _, out("v22") _, out("v23") _,
                    out("v24") _, out("v25") _, out("v26") _, out("v27") _, out("v28") _,
                    out("v29") _, out("v30") _, out("v31") _, out("x9") _, out("x10") _, out("x11") _, out("x12") _, out("x13") _, out("x14") _, out("x15") _, out("x16") _,
                );
                std::hint::black_box(&buf);
            });
            (($name), s * 1e9 / (ITERS as f64 / 10.0 * $per as f64))
        }};
    }

    pub fn run() {
        let npc = ns_per_cycle();
        println!("clock: {:.3} GHz", 1.0 / npc);
        let results = [
            probe!("pmull (16 indep)", 16, concat!(
                "pmull v16.1q, v0.1d, v1.1d\n", "pmull2 v17.1q, v0.2d, v1.2d\n",
                "pmull v18.1q, v2.1d, v3.1d\n", "pmull2 v19.1q, v2.2d, v3.2d\n",
                "pmull v20.1q, v4.1d, v5.1d\n", "pmull2 v21.1q, v4.2d, v5.2d\n",
                "pmull v22.1q, v6.1d, v7.1d\n", "pmull2 v23.1q, v6.2d, v7.2d\n",
                "pmull v24.1q, v0.1d, v2.1d\n", "pmull2 v25.1q, v0.2d, v2.2d\n",
                "pmull v26.1q, v1.1d, v3.1d\n", "pmull2 v27.1q, v1.2d, v3.2d\n",
                "pmull v28.1q, v4.1d, v6.1d\n", "pmull2 v29.1q, v4.2d, v6.2d\n",
                "pmull v30.1q, v5.1d, v7.1d\n", "pmull2 v31.1q, v5.2d, v7.2d\n",
            )),
            probe!("eor (16 indep)", 16, concat!(
                ".irp r,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31\n",
                "eor v\\r\\().16b, v0.16b, v1.16b\n", ".endr\n",
            )),
            probe!("eor3 (16 indep)", 16, concat!(
                ".irp r,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31\n",
                "eor3 v\\r\\().16b, v0.16b, v1.16b, v2.16b\n", ".endr\n",
            )),
            probe!("ldr q (16)", 16, concat!(
                "ldr q16, [{p}]\n", "ldr q17, [{p}, #16]\n", "ldr q18, [{p}, #32]\n", "ldr q19, [{p}, #48]\n",
                "ldr q20, [{p}, #64]\n", "ldr q21, [{p}, #80]\n", "ldr q22, [{p}, #96]\n", "ldr q23, [{p}, #112]\n",
                "ldr q24, [{p}, #128]\n", "ldr q25, [{p}, #144]\n", "ldr q26, [{p}, #160]\n", "ldr q27, [{p}, #176]\n",
                "ldr q28, [{p}, #192]\n", "ldr q29, [{p}, #208]\n", "ldr q30, [{p}, #224]\n", "ldr q31, [{p}, #240]\n",
            )),
            probe!("ldp q (8 = 16 regs)", 16, concat!(
                "ldp q16, q17, [{p}]\n", "ldp q18, q19, [{p}, #32]\n", "ldp q20, q21, [{p}, #64]\n",
                "ldp q22, q23, [{p}, #96]\n", "ldp q24, q25, [{p}, #128]\n", "ldp q26, q27, [{p}, #160]\n",
                "ldp q28, q29, [{p}, #192]\n", "ldp q30, q31, [{p}, #224]\n",
            )),
            probe!("ld1 x4 (4 = 16 regs)", 16, concat!(
                "ld1 {{v16.16b-v19.16b}}, [{p}]\n",
                "ld1 {{v20.16b-v23.16b}}, [{p}]\n",
                "ld1 {{v24.16b-v27.16b}}, [{p}]\n",
                "ld1 {{v28.16b-v31.16b}}, [{p}]\n",
            )),
            probe!("pmull+eor3 1:1 (per op)", 16, concat!(
                "pmull v16.1q, v0.1d, v1.1d\n", "eor3 v24.16b, v0.16b, v1.16b, v2.16b\n",
                "pmull2 v17.1q, v0.2d, v1.2d\n", "eor3 v25.16b, v0.16b, v1.16b, v3.16b\n",
                "pmull v18.1q, v2.1d, v3.1d\n", "eor3 v26.16b, v0.16b, v1.16b, v4.16b\n",
                "pmull2 v19.1q, v2.2d, v3.2d\n", "eor3 v27.16b, v0.16b, v1.16b, v5.16b\n",
                "pmull v20.1q, v4.1d, v5.1d\n", "eor3 v28.16b, v0.16b, v1.16b, v6.16b\n",
                "pmull2 v21.1q, v4.2d, v5.2d\n", "eor3 v29.16b, v0.16b, v1.16b, v7.16b\n",
                "pmull v22.1q, v6.1d, v7.1d\n", "eor3 v30.16b, v0.16b, v2.16b, v3.16b\n",
                "pmull2 v23.1q, v6.2d, v7.2d\n", "eor3 v31.16b, v0.16b, v2.16b, v4.16b\n",
            )),
            probe!("ldr q + pmull 1:1 (per pair)", 8, concat!(
                "ldr q16, [{p}]\n", "pmull v24.1q, v0.1d, v1.1d\n",
                "ldr q17, [{p}, #16]\n", "pmull2 v25.1q, v0.2d, v1.2d\n",
                "ldr q18, [{p}, #32]\n", "pmull v26.1q, v2.1d, v3.1d\n",
                "ldr q19, [{p}, #48]\n", "pmull2 v27.1q, v2.2d, v3.2d\n",
                "ldr q20, [{p}, #64]\n", "pmull v28.1q, v4.1d, v5.1d\n",
                "ldr q21, [{p}, #80]\n", "pmull2 v29.1q, v4.2d, v5.2d\n",
                "ldr q22, [{p}, #96]\n", "pmull v30.1q, v6.1d, v7.1d\n",
                "ldr q23, [{p}, #112]\n", "pmull2 v31.1q, v6.2d, v7.2d\n",
            )),
            probe!("3 ldr q + 4 simd (per group)", 4, concat!(
                ".rept 4\n",
                "ldr q16, [{p}]\n", "ldr q17, [{p}, #16]\n", "ldr q18, [{p}, #32]\n",
                "pmull v24.1q, v0.1d, v1.1d\n", "pmull2 v25.1q, v2.2d, v3.2d\n",
                "eor3 v26.16b, v0.16b, v1.16b, v2.16b\n", "eor3 v27.16b, v4.16b, v5.16b, v6.16b\n",
                ".endr\n",
            )),
            probe!("ldp x (8 pairs=16B each)", 8, concat!(
                "ldp x9, x10, [{p}]\n", "ldp x11, x12, [{p}, #16]\n", "ldp x13, x14, [{p}, #32]\n", "ldp x15, x16, [{p}, #48]\n",
                "ldp x9, x10, [{p}, #64]\n", "ldp x11, x12, [{p}, #80]\n", "ldp x13, x14, [{p}, #96]\n", "ldp x15, x16, [{p}, #112]\n",
            )),
            probe!("3 ldr q + 1 ldp x (per group)", 4, concat!(
                ".rept 4\n",
                "ldr q16, [{p}]\n", "ldr q17, [{p}, #16]\n", "ldr q18, [{p}, #32]\n", "ldp x9, x10, [{p}, #48]\n",
                ".endr\n",
            )),
            probe!("ldnp q (8 = 16 regs)", 16, concat!(
                "ldnp q16, q17, [{p}]\n", "ldnp q18, q19, [{p}, #32]\n", "ldnp q20, q21, [{p}, #64]\n",
                "ldnp q22, q23, [{p}, #96]\n", "ldnp q24, q25, [{p}, #128]\n", "ldnp q26, q27, [{p}, #160]\n",
                "ldnp q28, q29, [{p}, #192]\n", "ldnp q30, q31, [{p}, #224]\n",
            )),
            probe!("ld4 .2d (4 = 16 regs)", 16, concat!(
                "ld4 {{v16.2d-v19.2d}}, [{p}]\n", "ld4 {{v20.2d-v23.2d}}, [{p}]\n",
                "ld4 {{v24.2d-v27.2d}}, [{p}]\n", "ld4 {{v28.2d-v31.2d}}, [{p}]\n",
            )),
            probe!("ld2 .2d (8 = 16 regs)", 16, concat!(
                "ld2 {{v16.2d-v17.2d}}, [{p}]\n", "ld2 {{v18.2d-v19.2d}}, [{p}]\n",
                "ld2 {{v20.2d-v21.2d}}, [{p}]\n", "ld2 {{v22.2d-v23.2d}}, [{p}]\n",
                "ld2 {{v24.2d-v25.2d}}, [{p}]\n", "ld2 {{v26.2d-v27.2d}}, [{p}]\n",
                "ld2 {{v28.2d-v29.2d}}, [{p}]\n", "ld2 {{v30.2d-v31.2d}}, [{p}]\n",
            )),
            probe!("ldr q streaming 4KiB (per load)", 256, concat!(
                ".set off, 0\n", ".rept 256\n", "ldr q16, [{p}, #off]\n", ".set off, off+16\n", ".endr\n",
            )),
            probe!("ldr d (16)", 16, concat!(
                ".set off, 0\n", ".rept 16\n", "ldr d16, [{p}, #off]\n", ".set off, off+8\n", ".endr\n",
            )),
            probe!("NH-like: 8 ldp + 20 simd (per blk)", 1, concat!(
                "ldp q16, q17, [{p}]\n", "ldp q18, q19, [{p}, #32]\n",
                "ldp q20, q21, [{p}, #64]\n", "ldp q22, q23, [{p}, #96]\n",
                "ldp q24, q25, [{p}, #128]\n", "ldp q26, q27, [{p}, #160]\n",
                "ldp q28, q29, [{p}, #192]\n", "ldp q30, q31, [{p}, #224]\n",
                "eor v16.16b, v16.16b, v24.16b\n", "eor v20.16b, v20.16b, v28.16b\n",
                "pmull v24.1q, v16.1d, v20.1d\n", "pmull2 v28.1q, v16.2d, v20.2d\n",
                "eor3 v0.16b, v0.16b, v24.16b, v28.16b\n",
                "eor v17.16b, v17.16b, v25.16b\n", "eor v21.16b, v21.16b, v29.16b\n",
                "pmull v25.1q, v17.1d, v21.1d\n", "pmull2 v29.1q, v17.2d, v21.2d\n",
                "eor3 v1.16b, v1.16b, v25.16b, v29.16b\n",
                "eor v18.16b, v18.16b, v26.16b\n", "eor v22.16b, v22.16b, v30.16b\n",
                "pmull v26.1q, v18.1d, v22.1d\n", "pmull2 v30.1q, v18.2d, v22.2d\n",
                "eor3 v2.16b, v2.16b, v26.16b, v30.16b\n",
                "eor v19.16b, v19.16b, v27.16b\n", "eor v23.16b, v23.16b, v31.16b\n",
                "pmull v27.1q, v19.1d, v23.1d\n", "pmull2 v31.1q, v19.2d, v23.2d\n",
                "eor3 v3.16b, v3.16b, v27.16b, v31.16b\n",
            )),
            probe!("NH-like x4 unrolled (per blk)", 4, concat!(".rept 4\n",
                "ldp q16, q17, [{p}]\n", "ldp q18, q19, [{p}, #32]\n",
                "ldp q20, q21, [{p}, #64]\n", "ldp q22, q23, [{p}, #96]\n",
                "ldp q24, q25, [{p}, #128]\n", "ldp q26, q27, [{p}, #160]\n",
                "ldp q28, q29, [{p}, #192]\n", "ldp q30, q31, [{p}, #224]\n",
                "eor v16.16b, v16.16b, v24.16b\n", "eor v20.16b, v20.16b, v28.16b\n",
                "pmull v24.1q, v16.1d, v20.1d\n", "pmull2 v28.1q, v16.2d, v20.2d\n",
                "eor3 v0.16b, v0.16b, v24.16b, v28.16b\n",
                "eor v17.16b, v17.16b, v25.16b\n", "eor v21.16b, v21.16b, v29.16b\n",
                "pmull v25.1q, v17.1d, v21.1d\n", "pmull2 v29.1q, v17.2d, v21.2d\n",
                "eor3 v1.16b, v1.16b, v25.16b, v29.16b\n",
                "eor v18.16b, v18.16b, v26.16b\n", "eor v22.16b, v22.16b, v30.16b\n",
                "pmull v26.1q, v18.1d, v22.1d\n", "pmull2 v30.1q, v18.2d, v22.2d\n",
                "eor3 v2.16b, v2.16b, v26.16b, v30.16b\n",
                "eor v19.16b, v19.16b, v27.16b\n", "eor v23.16b, v23.16b, v31.16b\n",
                "pmull v27.1q, v19.1d, v23.1d\n", "pmull2 v31.1q, v19.2d, v23.2d\n",
                "eor3 v3.16b, v3.16b, v27.16b, v31.16b\n",
                ".endr\n",
            )),
            probe!("16 ldr q only, 1 blk (per blk)", 1, concat!(
                "ldp q16, q17, [{p}]\n", "ldp q18, q19, [{p}, #32]\n",
                "ldp q20, q21, [{p}, #64]\n", "ldp q22, q23, [{p}, #96]\n",
                "ldp q24, q25, [{p}, #128]\n", "ldp q26, q27, [{p}, #160]\n",
                "ldp q28, q29, [{p}, #192]\n", "ldp q30, q31, [{p}, #224]\n",
            )),
            probe!("8 pmull + 8 mov (per pmull)", 8, concat!(
                "pmull v16.1q, v0.1d, v1.1d\n", "mov v24.16b, v2.16b\n",
                "pmull2 v17.1q, v0.2d, v1.2d\n", "mov v25.16b, v3.16b\n",
                "pmull v18.1q, v2.1d, v3.1d\n", "mov v26.16b, v4.16b\n",
                "pmull2 v19.1q, v2.2d, v3.2d\n", "mov v27.16b, v5.16b\n",
                "pmull v20.1q, v4.1d, v5.1d\n", "mov v28.16b, v6.16b\n",
                "pmull2 v21.1q, v4.2d, v5.2d\n", "mov v29.16b, v7.16b\n",
                "pmull v22.1q, v6.1d, v7.1d\n", "mov v30.16b, v0.16b\n",
                "pmull2 v23.1q, v6.2d, v7.2d\n", "mov v31.16b, v1.16b\n",
            )),
            probe!("8 pmull only (per pmull)", 8, concat!(
                "pmull v16.1q, v0.1d, v1.1d\n", "pmull2 v17.1q, v0.2d, v1.2d\n",
                "pmull v18.1q, v2.1d, v3.1d\n", "pmull2 v19.1q, v2.2d, v3.2d\n",
                "pmull v20.1q, v4.1d, v5.1d\n", "pmull2 v21.1q, v4.2d, v5.2d\n",
                "pmull v22.1q, v6.1d, v7.1d\n", "pmull2 v23.1q, v6.2d, v7.2d\n",
            )),
            probe!("chain mix 16 ldr + 20 simd (per blk)", 4, concat!(".rept 4\n", "ldr q16, [{p}, #0]\n", "ldr q20, [{p}, #64]\n", "ldr q24, [{p}, #128]\n", "ldr q25, [{p}, #192]\n", "eor3 v24.16b, v8.16b, v16.16b, v24.16b\n", "eor3 v25.16b, v12.16b, v20.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v0.16b, v0.16b, v26.16b, v27.16b\n", "ldr q17, [{p}, #16]\n", "ldr q21, [{p}, #80]\n", "ldr q24, [{p}, #144]\n", "ldr q25, [{p}, #208]\n", "eor3 v24.16b, v9.16b, v17.16b, v24.16b\n", "eor3 v25.16b, v13.16b, v21.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v1.16b, v1.16b, v26.16b, v27.16b\n", "ldr q18, [{p}, #32]\n", "ldr q22, [{p}, #96]\n", "ldr q24, [{p}, #160]\n", "ldr q25, [{p}, #224]\n", "eor3 v24.16b, v10.16b, v18.16b, v24.16b\n", "eor3 v25.16b, v14.16b, v22.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v2.16b, v2.16b, v26.16b, v27.16b\n", "ldr q19, [{p}, #48]\n", "ldr q23, [{p}, #112]\n", "ldr q24, [{p}, #176]\n", "ldr q25, [{p}, #240]\n", "eor3 v24.16b, v11.16b, v19.16b, v24.16b\n", "eor3 v25.16b, v15.16b, v23.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v3.16b, v3.16b, v26.16b, v27.16b\n",  ".endr\n")),
            probe!("chain mix 8 ldp + 20 simd (per blk)", 4, concat!(".rept 4\n", "ldp q16, q17, [{p}, #0]\n", "ldp q20, q21, [{p}, #64]\n", "ldp q24, q28, [{p}, #128]\n", "ldp q25, q29, [{p}, #192]\n", "eor3 v24.16b, v8.16b, v16.16b, v24.16b\n", "eor3 v25.16b, v12.16b, v20.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v0.16b, v0.16b, v26.16b, v27.16b\n", "eor3 v28.16b, v9.16b, v17.16b, v28.16b\n", "eor3 v29.16b, v13.16b, v21.16b, v29.16b\n", "pmull v26.1q, v28.1d, v29.1d\n", "pmull2 v27.1q, v28.2d, v29.2d\n", "eor3 v1.16b, v1.16b, v26.16b, v27.16b\n", "ldp q18, q19, [{p}, #32]\n", "ldp q22, q23, [{p}, #96]\n", "ldp q24, q28, [{p}, #160]\n", "ldp q25, q29, [{p}, #224]\n", "eor3 v24.16b, v10.16b, v18.16b, v24.16b\n", "eor3 v25.16b, v14.16b, v22.16b, v25.16b\n", "pmull v26.1q, v24.1d, v25.1d\n", "pmull2 v27.1q, v24.2d, v25.2d\n", "eor3 v2.16b, v2.16b, v26.16b, v27.16b\n", "eor3 v28.16b, v11.16b, v19.16b, v28.16b\n", "eor3 v29.16b, v15.16b, v23.16b, v29.16b\n", "pmull v26.1q, v28.1d, v29.1d\n", "pmull2 v27.1q, v28.2d, v29.2d\n", "eor3 v3.16b, v3.16b, v26.16b, v27.16b\n",  ".endr\n")),
        ];
        for (name, ns) in results {
            println!("{name:32} {:.3} cycles each ({:.2}/cycle)", ns / npc, npc / ns);
        }
    }
}

fn main() {
    #[cfg(target_arch = "aarch64")]
    probes::run();
}

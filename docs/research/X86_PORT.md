# x86 port: status of the September 2026 improvements on x86

> Research notes kept for their reasoning. The raw result files they cite (`results/`) were removed before publication; current measurements are in [BENCHMARKS.md](../BENCHMARKS.md).

Status 2026-09-28. The x86 backends were finished, and the AVX-512 backend
was tuned, on real hardware. That hardware was an AWS `c8a.2xlarge` with an
AMD EPYC 9R45 (Zen 5, 4.49 GHz). Measurements are in the
initial and
optimized checkpoints.
Hash outputs did not change. The earlier version of this file listed what x86
lacked after the NEON work; the table below says what became of each item.

## The backends

- `x86/avx512.rs`: a dedicated AVX-512 backend. It requires AVX-512 F, BW,
  VL and VBMI2, VPCLMULQDQ and BMI2. Every CPU with 512-bit VPCLMULQDQ has
  these.
- `x86/mod.rs`: the generic kernel, now instantiated for SSE (with SSSE3)
  and AVX2 only, plus shared PCLMUL helpers.
- `x86/aes.rs`: AES-NI, and 512-bit VAES for counter mode.

## The NEON-only items

| Item | x86 now | Where |
|---|---|---|
| 1. Two chunks sharing key loads | Tried and rejected on Zen 5: slower in every regime, including two distant streams. The single-chunk kernel keeps one accumulator per block and prefetches 4 KiB ahead for inputs of 512 KiB to 32 MiB; from memory the hardware prefetcher alone is faster. | `avx512::bulk`, `chunks` |
| 2. One kernel for the final partial chunk | Yes, for AVX-512: `tail`, also used for one-shot inputs under 8 KiB | `avx512::tail` |
| 3. 32–64-byte shortcut (S2) | Yes, all x86 backends. AVX-512 uses one masked load and four vector multiplies; SSE/AVX2 use `pshufb` over the last 32 bytes. The coefficients are prepared with PCLMUL (512-bit for AVX-512). | `avx512::medium`, `x86::medium`, `s2_prepare` |
| 4. Per-position weights up to 1 KiB | Superseded on AVX-512 by a fresh-chunk path with compile-time zero sums and a single packed multiply. Weights would cost four extra multiplies per position, and the multiply is the scarce resource. | `avx512::small` |
| 5. Hand-written AES | Yes: AES-NI MAC mask (0-byte MAC 17.6 → 5.4 ns); 512-bit VAES counter mode and a shorter key-schedule chain (key setup 128 → 100 ns) | `x86/aes.rs` |
| 6. Avalanche multiply | Yes: PCLMUL for all x86 backends (0-byte avalanche 19.4 → 5.6 ns) | `x86::mul_key` |

## Also new for AVX-512

- A partial last block is read with masked loads instead of being copied to
  the stack.
- Streaming input is copied into a 64-byte-aligned buffer with whole
  64-byte stores, so the kernels' loads forward from them. This also removes
  the store-forwarding stall at 15 and 31 bytes that the "word stores"
  partial win below targeted.
- Between `update` and `finalize` the chunk is kept as two sums (`h0`, `h1`)
  plus the previous block, instead of eleven lane sums.
- `h0 R + h1 R2 + L T` is one packed multiply by `(R, T, R2, 0)`, stored in
  `Key::pk` together with `x^64 (R, T, R2, 0)`, so the products reduce with
  one fold of a single word.
- `Key::pk` and `Key::s2` follow the table, so their 64-byte loads are
  aligned.
- A message of 65 bytes to 1 KiB goes from `Key::hash` or `Hasher::finalize`
  to its own kernel, `group`, skipping the generic `finish` (9–16% faster
  one-shot, 7–12% for the MAC;
  results).
  Later x86 experiments on candidate algorithms are in
  [CANDIDATES.md §10](CANDIDATES.md#10-x86-experiments-2026-09-28).

## Second round, 2026-09-29

Results;
[progress log](../../progress.md). Outputs unchanged.

- **Key construction** fills the caller's return slot (`expand_into`,
  `init_into`) instead of moving a 9 KB local, and makes its eager AES
  blocks in one VAES batch (`aes::eager_wide`): setup 102 → 60 ns.
- **1–31 bytes.** The key stores `x^64 A` and `x^64 B` (`Key::sk`), so
  `x0 A + x1 B` needs four independent products and one fold. On AVX-512,
  `avx512::short_msg` reads `[X0 | X1]` with one 256-bit masked load and has
  no length branches: 2.7 ns, and 2.9 ns for random lengths, where the old
  `partial16` branches mispredicted (11.6 ns). AVX2 has an equivalent
  (`x86::short_avx2`, dword-masked load); `Hasher::finalize` avoids it, since
  masked loads do not forward from `update`'s stores. The SSE backend uses
  the same branchless loads when the CPU has AVX2 (`x86::short_avx2_x`),
  which is the case for its main users: Haswell to Coffee Lake and Zen 1/2,
  which lack VPCLMULQDQ. Random 1–31 B go from 11.5 to 5.4 ns there
  (forced on Zen 5). Without AVX2 it keeps the old loads with the single
  fold.
- **32–64 bytes.** On AVX-512, `L T` is one 256-bit multiply: 4.5 → 3.6 ns.
  AVX2 runs the affine form with 256-bit multiplies (`x86::medium_avx2`):
  8.7 → 4.9 ns one-shot. The hasher keeps the 128-bit form there, because
  its loads must forward from `update`'s stores.
- **SSE/AVX2 65 bytes to 8 KiB.** The padded last block is built from whole
  32-byte stores, which AVX2's loads forward from. Final-chunk closes sum
  their planes with shifts and do one `R2` product (`Wide64`).
- The key is now 9472 bytes.
- **Open:** AVX2's streamed 65–300 bytes still stall on the forwarding of
  the pending-buffer copy (`memcpy`'s overlapping stores). A fix tried on
  Zen 5 slowed other paths. Whether it matters on a real AVX2 CPU is
  untested.

## What limits x86 speed on Zen 5

- **The multiply is microcoded:** `vpclmulqdq` is four micro-ops and issues
  every other cycle at any vector width. A 128-byte block (eight 64-bit
  products, two zmm multiplies) therefore costs at least four cycles, or
  about 144 GB/s.
- **Where we stand:** RaijuHash reaches 130–131 GB/s from L2
  (133 GB/s at 256 KiB after the shorter chunk close of 2026-09-29). From L3,
  it is near the single-core bandwidth of about 100–115 GB/s.
- **Front-end cost:** the multiply's micro-ops compete with other instructions
  for the front end. For medium inputs (1–8 KiB) the instruction count
  matters as well as the multiply count.
- **Unaligned loads halve load throughput:** aligned 64-byte loads run two per
  cycle, unaligned ones one. Streaming prefetch into L1 (T0) at 4 KiB beat
  every L2 (T1) and longer-distance variant tried.

## Partial wins from the M1 work, re-examined

- **Word stores for a short first `update`:** superseded on AVX-512 by the
  aligned 64-byte buffer copy. It is not needed.
- **S1 short path:** not tried. Short inputs already take 5–8 ns.
- **F planes in general registers:** not applicable. AVX-512 has 32 vector
  registers and the bulk kernel does not spill.

## Verification done on x86 hardware

1. **Tests on a real CPU:** `cargo test --release -p raijuhash --all-features`
   passes on the EPYC 9R45. That covers SSE, AVX2, AVX-512, the emulated AVX2
   test backend and portable, with runtime detection, with
   `-C target-cpu=native`, and with `--no-default-features`.
2. **The session's test additions:** streaming pieces of exactly 1024 and
   2048 bytes, because a full group now stays buffered.
3. **Constant-time check:** inspected in the x86-64 release build.
   `Mac::verify` and `MacHasher::verify` XOR and OR the tag halves, `sete`,
   then pass the bit through `subtle`'s out-of-line barrier before branching
   (SPEC §6.4).
4. **Stale claim:** the README's throughput claim was replaced with measured
   figures.

## Still open

- **Intel AVX-512 CPUs** (Ice Lake, Sapphire Rapids and later): not measured.
  Their `vpclmulqdq` throughput differs from Zen 5, so the bulk ceiling and
  the prefetch and one-chunk decisions must be re-checked there.
- **AVX2-only CPUs** (Zen 3, Alder Lake): not measured. On Zen 5 the forced
  AVX2 and SSE backends reach 66 and 34 GB/s, bound by multiply width. They
  also lack the AVX-512 extras: masked tails, the fresh-chunk path, the
  packed output multiply and the two-sum streaming state.
- **Layout stall:** in about 1 of 60 process layouts, the benchmark harness
  showed a false 4 KiB-aliasing dependency between its own stack reload and
  the hasher's buffer store. That run was about 3× slower for short streamed
  inputs. There is no general fix inside the library. See the optimized
  results.

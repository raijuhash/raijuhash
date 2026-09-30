# Candidate designs on AMD Zen 5

> Research notes kept for their reasoning. The raw result files they cite (`results/`) were removed before publication; current measurements are in [BENCHMARKS.md](../BENCHMARKS.md).

Status 2026-09-28. After the x86 port
(results), every
family in [CANDIDATES.md](CANDIDATES.md) and the relevant public literature
was checked for one question: could any design hash much faster than
RaijuHash v1 on this CPU? Measurements are from an AMD EPYC 9R45 (Zen 5,
4.49 GHz, AWS `c8a.2xlarge`) with Rust 1.98.1 and `-C target-cpu=native`.
The probe and prototype programs were scratch code, not in the repository.
Their method and results are recorded here.

**Answer:**

- **No known 128-bit universal hash is much faster here.** The best
  alternative found, a hybrid of carryless and integer multiplication, gains
  about 13% in a bare kernel and would need a new algorithm and new proofs.
- **Anything larger gives something up:** 64-bit output (Chain64) or more
  cores (`hash_parallel`).
- **One real defect turned up.** Software prefetch slowed inputs larger
  than L3. It was fixed.

## 1. What the CPU offers

Each instruction was issued back to back (eight independent chains), and
then mixed with `vpclmulqdq` to see whether they share execution resources.

| Instruction (512-bit unless noted) | Issue rate | Runs alongside `vpclmulqdq`? |
|---|---:|---|
| `vpclmulqdq` (any width, VEX or EVEX) | 0.5 per cycle, latency 5; microcoded, 4 ops | — |
| `vpmuludq`, `vpmullq` | 2 per cycle | yes: 4 `vpclmulqdq` + 16 `vpmuludq` take 8 cycles |
| `vpmadd52luq`, `vpdpwssd`, `vpdpbusd` | about 1.8 per cycle | yes |
| `vpmaddwd`, `vgf2p8mulb`, `vgf2p8affineqb` | 2 per cycle | not tested |
| `mulx` (scalar 64×64→128) | 1.5 per cycle | yes |
| `vpternlogq` / `vpxorq`, `vpaddq` / shifts | 3.6 / 4 / 2 per cycle | yes |
| `vaesenc` | 2 per cycle, latency 4 | — |
| 64-byte loads | 2 per cycle aligned, 1 unaligned | — |

The integer multipliers have more raw capacity than the carryless one: 16
32×32 products per cycle against 2 64×64. But the carryless multiply's four
ops already fill vector-pipe slots. RaijuHash's block loop uses about three
of the four vector slots per cycle, so little is left for other work (§4).

## 2. The families of CANDIDATES.md

Products are counted per 128 bytes at a 128-bit bound. A 512-bit carryless
multiply yields 2 products per cycle and `vpmuludq` 16. The "floor" counts
only the multiplier.

| Family | Products per 128 B | Floor on Zen 5 | Verdict |
|---|---|---:|---|
| RaijuHash v1 (chain code) | 8.125 carryless | 31.5 B/cycle | Measured 29 in the full kernel (130 GB/s from L2); the bare loop runs at 32.0. |
| U8 / U16 / U32 (wider EHC) | 9 / 8.5 / 8.25 carryless | 28 / 30 / 31 B/cycle | Dominated: more products than the chain code, plus parity work. |
| M32 (Multimixer-128) | 32 integer (0.25 per byte) | 64 B/cycle | Limited by instruction count, not the multiplier. A Multimixer-shaped kernel (key add, circulant sums, two `vpmuludq` per 64 B) measured **21.9 B/cycle (98 GB/s)**, below RaijuHash. |
| P256 (GF(2^256) polynomial) | ≥ 44 carryless (Karatsuba plus reduction) | ≤ 6 B/cycle | Slower by 4–5× in bulk; its merit is the key size. |
| L128 (field-linear) | 24 carryless | 10.7 B/cycle | Slower by 3×. |
| I4 (four NH-32 streams) | 64 integer plus key adds | ≤ 32 B/cycle before overhead | HalftimeHash40, the best-engineered public relative, measured 59–65 GB/s (§3). |
| D2 (two carryless NH streams) | 16 carryless | 16 B/cycle | Slower by 2×. |
| Pyramid128, in-register key rolling | as RaijuHash | — | Rejected in review for security. They also target key traffic, which is not the limit on Zen 5. |
| Chain64 | 4 carryless | ~64 B/cycle (load-bound) | About 2× faster, but with a 64-bit output: outside the 128-bit contract. |
| SuperChain256 | as RaijuHash | — | **Measured slower.** Two chunks in step ran at 118 against 128 GB/s from L2, and 82 against 104 from L3. The premise of two 512-bit VPCLMUL pipes is false on Zen 5. |
| Permuted Chain | as RaijuHash plus 2 AES rounds | — | Rejected in review for security. It is a MAC shape, not a speed idea. |

The cross-candidate ideas:

- **Lazy parameter expansion:** implemented (`Key::from_seed` generates rows
  on first use).
- **Hybrid short/medium/bulk families:** implemented in v1 (the short path,
  the 32–64 B affine form, and the bulk path).
- **Message batching:** no gain for single long messages, which are
  multiply-bound. An API hashing several short messages per call could raise
  their throughput, but that is a new API, not a faster hash.
- **Many-core composition:** already available as `Key::hash_parallel`. With
  8 threads it measured 143 / 195 / 237 GB/s at 16 MiB (2 / 4 / 8 threads),
  2.2× one thread. Beyond L3 the instance's memory bandwidth caps even 8
  threads at about 55 GB/s. Below about 4 MiB it is slower than one thread,
  because it spawns its threads on every call; a persistent pool would fix
  that.

## 3. Public literature

- **Carryless NH:** CLHash (Lemire and Kaser, 2016) and UMASH use the same
  64-bit carryless products as RaijuHash. For a 128-bit bound
  they need more products per byte than the chain code (UMASH's 128-bit
  fingerprint is two 64-bit hashes).
- **Integer NH:** NH, UMAC's NH-Toeplitz (Black et al., 1999), VHASH and
  Adiantum's NH need 64 32-bit products per 128 B for 2^-128. A scalar
  VHASH-style NH-64 stream with `mulx` measured 3.7 B/cycle on its own here.
- **HalftimeHash** (Apple, 2021) is encode-hash-combine over integer NH,
  with tree hashing. The public Rust crate `halftime` 0.1.1 was measured on
  this CPU with key setup subtracted, in GB/s:

  | Size | HH16 | HH24 | HH32 | HH40 | RaijuHash |
  |---|---:|---:|---:|---:|---:|
  | 64 KiB | 112 | 96 | 73 | 59 | 128 |
  | 256 KiB | 115 | 106 | 80 | 65 | 131 |
  | 16 MiB | 85 | 71 | 69 | 56 | 102 |

  Every variant is slower than RaijuHash, including those with weaker
  bounds. HalftimeHash is also AU rather than AXU, with 16–40-byte outputs.
- **Multimixer-128** (Ghosh, Amiri Eliasi and Daemen, ToSC 2023) is 2^-127
  Δ-universal with 8 32-bit products per 32 B. It is fast on ARMv7 NEON
  (1.2–1.8 cycles per byte on Cortex-A7). On Zen 5 it is instruction-bound
  (§2).
- **Encode-hash-combine** (Nandi, FSE 2014) bounds how few multiplications a
  universal hash can use. The chain code reaches a 2^-128 bound with one
  64×64 product per 16 bytes (plus one per chunk), the NH pairing density.
  No public construction found uses fewer carryless products per byte at
  that bound.
- **Polynomial hashing:** GHASH/POLYVAL, Poly1305 and BRW polynomials
  (Chakraborty, Ghosh and Sarkar) need at least about 1.5 carryless products
  per 16 B even with BRW, plus reductions. They are slower in bulk.
- **AES-round designs:** Pelican-style designs and VAES at 2 per cycle would
  land near 32 B/cycle. Their security rests on bounds for the differential
  probability of reduced-round AES rather than unconditional universality.
  Tachyon, an experimental VAES hash, claims no bound at all.
- **GFNI, VNNI, IFMA:** linear hashing over GF(2) or GF(2^8) with
  `vgf2p8affineqb`/`vgf2p8mulb` needs many operations per secure byte. NH
  over 16-bit words with `vpdpwssd` needs 8 instances for 2^-128, and 52-bit
  IFMA needs two instructions per product. All of these fill the same
  vector pipes and come out slower.

## 4. The one design that gains: a carryless + integer hybrid

The integer multiplier runs alongside the carryless one, so a design could
split each chunk between the two. It would combine a carryless NH part and a
Multimixer- or NH-Toeplitz part with independent keys. Projecting each part
into GF(2^128) keeps the bound near the larger of the two.

A prototype loop mixed RaijuHash's block kernel with the Multimixer-shaped
integer kernel, with data in L1 (bytes per cycle):

| Mix per iteration | B/cycle | GB/s |
|---|---:|---:|
| 128 B carryless only | 32.0 | 144 |
| 128 B integer only | 21.9 | 98 |
| 128 B carryless + 64 B integer | 34.8 | 156 |
| **256 B carryless + 128 B integer** | **36.4** | **164** |
| 256 B carryless + 256 B integer | 31.8 | 143 |
| 128 B carryless + 16 B scalar `mulx` NH | 18.8 | 85 |

The best mix is 13% above the carryless loop alone. Every op of the integer
part takes a vector-pipe slot the carryless part also needs, and scalar NH
starves the front end. After chunk-close overheads the real gain would be
smaller.

The costs are large:

- a new hash definition (v2) with new frozen vectors;
- a new security proof, including the Lean formalization the project
  requires;
- a different balance on other CPUs (the M1 NEON kernel would change too).

Not recommended for a 13% ceiling.

## 5. Found and fixed: prefetch beyond L3

The initial harness stopped at 16 MiB, so inputs streamed from memory had
not been measured. At 64 MiB–1 GiB RaijuHash ran at 38–44 GB/s. The 4 KiB
software prefetch that helps inputs coming from L3 costs about 15% when
streaming from memory, where the hardware prefetcher does better alone.

Prefetch now applies only to inputs of 512 KiB to 32 MiB. From memory,
RaijuHash reaches 42–53 GB/s for 64 MiB to 1 GiB.

## Sources

- J. Apple, [HalftimeHash: Modern Hashing without 64-bit Multipliers or Finite Fields](https://arxiv.org/abs/2104.08865), 2021; crate [`halftime`](https://docs.rs/halftime/latest/halftime/).
- K. Ghosh, P. Amiri Eliasi, J. Daemen, [Multimixer-128: Universal Keyed Hashing Based on Integer Multiplication](https://eprint.iacr.org/2023/1357), ToSC 2023.
- D. Lemire, O. Kaser, [Faster 64-bit universal hashing using carry-less multiplications](https://arxiv.org/abs/1503.03465), JCEN 2016.
- M. Nandi, On the minimum number of multiplications necessary for universal hash functions, FSE 2014 ([ePrint 2013/574](https://eprint.iacr.org/2013/574)).
- J. Black et al., UMAC: Fast and secure message authentication, CRYPTO 1999.
- D. Chakraborty, C. Ghosh, P. Sarkar, [ePrint 2016/1103](https://eprint.iacr.org/2016/1103) (BRW and Horner hierarchies).
- A. Yee, [Zen5's AVX512 Teardown](https://www.numberworld.org/blogs/2024_8_7_zen5_avx512_teardown/), 2024.
- [Tachyon](https://github.com/byt3forg3/Tachyon) (experimental VAES hash, no universality claim).

# Results: the initial RaijuHash implementation

> Research notes kept for their reasoning. The raw result files they cite (`results/`) were removed before publication; current measurements are in [BENCHMARKS.md](../BENCHMARKS.md).

> **Status, 2026-09-28.** The numbers below predate two later rounds of
> optimization:
> - M1 measurements and raw runs:
>   apple-m1-optimized-2026-09-27.md.
> - The x86 backends were then finished and tuned on an AMD EPYC 9R45
>   (Zen 5): initial
>   and optimized
>   checkpoints, and [X86_PORT.md](X86_PORT.md). See
>   [BENCHMARKS.md](../BENCHMARKS.md) for the current Zen 5 measurements.

Status as of 2026-09-27. Everything here was measured on the local Apple M1
(`MacBookAir10,1`, Firestorm P-core at 3.19 GHz, 128 KiB L1D, 12 MiB L2),
Rust 1.98.1, `RUSTFLAGS='-C target-cpu=native'`, release profile with thin
LTO.

The outcome is **RaijuHash** (`crates/raijuhash`, specification and proof in
[`crates/raijuhash/SPEC.md`](../../crates/raijuhash/SPEC.md)): a 128-bit keyed
almost-XOR-universal hash with an optional nonce-based MAC. It is written
from the plan in this directory and public papers.

## Headline numbers (M1, hot cache, same process)

RaijuHash is shown through its streaming `Hasher` (new hasher, one
`update`, finalize), the one-shot `Key::hash`, and its MAC API. Short inputs
are time per message; bulk inputs are throughput.

| Size | raijuhash `Hasher` | raijuhash `hash` | raijuhash MAC |
|---:|---:|---:|---:|
| 16 B | 4.8 ns | 3.7 ns | 16.6 ns |
| 64 B | 11.9 ns | 9.9 ns | 21.2 ns |
| 256 B | 15.9 ns | 10.9 ns | 23.1 ns |
| 1 KiB | 36.4 ns | 28.3 ns | 44.1 ns |
| 2 KiB | 52.2 ns | 50.1 ns | 64.3 ns |
| 4 KiB | 81.0 ns | 79.3 ns | 94.9 ns |
| 8 KiB | 59.9 GB/s | 60.1 GB/s | |
| 64 KiB | 65.2 GB/s | 65.3 GB/s | |
| 128 KiB | 57.2 GB/s | 56.5 GB/s | |
| 1 MiB | 53.7 GB/s | 53.6 GB/s | |
| 16 MiB | 49.9 GB/s | 50.1 GB/s | |

These are historical measurements. Reproduce current streaming and
one-shot timings with `cargo run --release -p benchmarks --bin quick --
16 64 256 1024 2048 4096 8192 65536`, using the flags above.

From 256 KiB up, data streams from L2 or beyond and the hash reaches the
memory-system limit (about 53 GB/s from L2). At 16–48 bytes, RaijuHash uses
one or two field multiplications.

Other properties:

| | raijuhash |
|---|---|
| AXU bound for messages up to `L` bytes | `(ceil(L/8192) + 1) / 2^128` |
| Key entropy / key struct | 8416 B / 8960 B |
| Key setup from a 128-bit seed | 851 ns |
| Streaming state | 1488 B |
| Carryless products per 128 B of bulk data | 8.125 |

## The construction in one paragraph

Each 128-byte block is an X row and a Y row of eight 64-bit words; lane `l`
pairs `X[l]` with `Y[l]`. Within a chunk of 64 blocks, each lane is
**chain-encoded**: encoded position `j` is `block[j-1] XOR block[j]` (zero
outside the chunk), keyed with its own table row, and hashed with carryless
NH (`clmul(x^a, y^b)`). A chain code has distance 2, so a change anywhere
touches two positions; combining positions with columns `(1, j)` and
`(0, 1)` (every pair invertible) lifts the 2^-64 NH bound to 2^-128 for the
chunk (encode-hash-combine). Chunks feed a bivariate polynomial over
GF(2^128), and the length is bound by an independent key. Messages below
32 bytes use one or two field multiplications.

Why it is faster: encoding and keying merge into one three-way XOR per
register (`EOR3`), so a block costs 8 XOR3 + 8 PMULL + 6 accumulate
operations instead of separate key XOR, parity XOR and extra products. With
`j = 8u + v` as the combination coefficient, sixteen running sums (eight
`B[v]` plus three bit-plane sums of `C[u]`) replace 65 accumulators, which
leaves registers to group each block's data loads.

## Microarchitecture findings (M1 Firestorm)

Measured with the probes in `experiments/src/bin/`:

- PMULL, EOR and EOR3 each issue 4 per cycle on the 4 SIMD pipes; SIMD
  register moves are mostly eliminated.
- **Loads cap at 3 per cycle of any width** (LDR q, LDP q and LD1 x4 all
  move 48 B/cycle), and overlap fully with SIMD work. Any NH-style hash reads
  at least one key byte per data byte, so the L1 floor is
  `(8 + 8r) / 3` cycles per 128 B for `r` key bytes per data byte:
  5.33 cycles at `r = 1`. raijuhash's kernel runs 5.6–5.8 cycles per 128 B
  (about 70 GB/s kernel-only).
- **From L2, each 128-byte line fill costs about 2 cycles that do not
  overlap loads**: data-only streaming reaches 86 GB/s, but with `r = 1` key
  loads the ceiling is about 55 GB/s whatever the arithmetic. This is why
  the hash is memory-bound above 128 KiB.
- **Page-offset hazards (16 KiB pages):** a store whose page offset the
  following loads approach stalls those loads (up to 2× slower). The bulk
  kernel therefore performs no stores in its loop; the outer accumulator
  lives in two general registers. Independently, any load stream starting
  within about 2 KiB after a 16 KiB page boundary runs slower for all code
  (even a plain XOR-of-loads loop: 62 vs 102 GB/s), so the comparison
  harness pins data at page offset 4096 for every hash.
- Non-temporal loads (LDNP) gave +3% at 8–16 MiB on repeated buffers but
  lost at 1 MiB and 64 MiB; not used.

## Candidate screening (plan families, NEON, 64 KiB hot, compression only)

| Candidate | cycles / 128 B | GB/s | Verdict |
|---|---:|---:|---|
| Plain carryless NH (only 2^-64, reference floor) | 5.7 | 71 | not a 128-bit hash |
| **Chain code, 64 blocks, u/v sums (raijuhash)** | **5.8** | **70** | selected; best L2 (53 GB/s) |
| Chain code, 16 blocks, 17 accumulators | 5.8 | 71 | L2 only 51.8 GB/s (no room to group loads) |
| Parity EHC U4 / U8 / U16 (independent keys) | 7.0–7.6 | 53–58 | loses: key XOR + parity XOR per register |
| D2: two independent NH streams | 10.5 | 38 | 16 products and 2× keys per 128 B |
| L128: field-linear chunks | 12.7 | 32 | 4 products per 16 bytes |
| M32: Multimixer-style integer mixing | 14.2 | 29 | adds, rotations and 8 products per 32 B |
| I4: four 32-bit integer NH streams | 17.4 | 23 | 4× key traffic |
| Aggregated polynomial (POLYVAL-like, compact key) | 37.9 | 11 | full field products per 16 B |
| P256 compact-key polynomial | — | — | strictly more field work per byte than the line above |

Kernels: `experiments/src/neon.rs`, `experiments/gen_asm.py`; harnesses:
`experiments/src/bin/{screen,multi,stream,micro}.rs`.

## Ideas tried and rejected

- **Sharing key words to beat the L2 limit** (y-keys shared across lanes or
  stripes, Toeplitz shifts along the chain, tabulated keys): each would cut
  key loads, the only lever left above 128 KiB, but each admits explicit
  linear cancellations with public small combination columns (for example
  two lanes differing only in `x` with equal differences cancel a shared
  `y`-key), and secret random columns would make the per-chunk fold cost
  several multiplications per block. None is provably 2^-128, so none was
  adopted.
- Key-then-encode (keys added before the chain code) to reach `r = 1`: needs
  a new proof and costs an extra XOR per register; the saving at L2 is under
  1.6%.
- Software prefetch, interleaving two input streams, keys-first load order:
  no gain or slower.
- A second copy of the key table to dodge cache conflicts: the conflicts
  turned out to be store and page-boundary effects instead; removed.
- Compiled intrinsics for the bulk loop: LLVM spills the accumulators (8–12%
  slower), so the loop is inline assembly built from assembler macros
  (`crates/raijuhash/src/neon_asm.rs`); everything else is intrinsics.

## Correctness

`crates/raijuhash/src/reference.rs` transcribes SPEC.md literally (bitwise
carryless products, explicit multiplication by each position index). Tests
check the NEON backends (with and without EOR3) and the portable backend
against it for every length 0–1100 and around every chunk and group boundary
up to 100 KB, every two-way split of short messages, fixed-size streaming
pieces from 1 byte to 9000, reset and repeated finalize, and MAC round trips.

## Gaps and next steps

- (Resolved 2026-09-28.) The x86-64 SIMD backends were written and then
  measured on Zen 5; see the optimized checkpoint. The expected
  multiply-bound behavior holds there: about 130 GB/s from L2.
- The security argument is a written proof, not a machine-checked one.
- Key setup is substantial, which matters when keys are not reused.

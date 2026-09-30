# SIMD and modern-CPU implementation plan

The user explicitly requested NEON, AVX-512, broad modern-CPU coverage, and any
SIMD optimization that helps. NEON/PMULL and AVX-512/VPCLMUL therefore receive
first priority. All backends implement the same specified function and produce
the same 128-bit output, regardless of vector width or update partition.

The [candidate portfolio](CANDIDATES.md) changes which instructions matter most.
The detailed eight-pair layout below is U8's worked example, not a shared layout
that every family must use. Before selecting a core, compare these kernel types:

| Candidate family | SIMD work to compare |
|---|---|
| U8/U16/U32 | Polynomial multiply, parity reduction, coefficient combination, and register pressure as the group grows. |
| M32 | Widening 32-bit integer multiply and the complete published add/carry schedule; field projection at boundaries. |
| I4 | Four independent widening-multiply/accumulate streams, input sharing, and four sets of key loads. |
| P256 | Wide field products, squaring/reduction, folded polynomial evaluation, and compact key access. |
| L128 | Parallel full-field products with one delayed reduction; 16-byte streaming units. |
| D2 | Regular independent carryless products with no parity or coefficient-matrix work. |

Feature checks must follow the instructions a candidate actually uses. For
example, integer SIMD can accelerate M32/I4 bodies even when the selected
portable field backend handles their much less frequent outer operations.

## Backend matrix

| Priority | Backend | Proposed work | Dispatch/acceptance condition |
|---|---|---|---|
| Primary | AArch64 NEON + PMULL | Pair low/high polynomial multiplies, keep independent stripes in flight, minimize lane movement, and combine XORs efficiently. | Check exact PMULL/intrinsic requirements; measure Apple Silicon and an Arm server. |
| Primary | x86-64 AVX-512 + VPCLMUL | Process four carryless products per vector instruction where the layout permits; pack parity work across stripes; use ternary XOR where profitable. | Require VPCLMUL and every AVX-512 subset actually used, with OS vector-state support. Measure Intel and AMD independently. |
| Coverage | x86-64 AVX2 + VPCLMUL | A 256-bit kernel for CPUs without usable AVX-512 and cases where it wins on latency/power behavior. | Require VPCLMUL as well as the required AVX2 operations. |
| Coverage | x86-64 SSE/PCLMUL and AVX/PCLMUL | Efficient 128-bit multiplication paths, short-message helpers, and narrower alternatives to wide-vector startup. | Detect PCLMUL and the encoding/features actually emitted. |
| Coverage | Portable scalar | Correct same-digest implementation; optimize software carryless products after the safe oracle is established. | Runs on supported targets without polynomial-multiply hardware. |
| Evaluate | Arm SVE2 with 128-bit-result polynomial multiply support | Pack independent products across the available vector length; compare PMULLB/PMULLT kernels to NEON. | Verify the polynomial crypto subextension, OS support, Rust/assembler support, and speed on real hardware. SVE2 alone is insufficient evidence. |
| Evaluate | x86 AVX10-capable CPUs | Reuse equivalent 256/512-bit polynomial kernels where supported; check compiler and feature enumeration. | Determine the exact usable instructions/vector widths from the CPU and OS. Do not select by the marketing name alone. |
| Evaluate | RISC-V scalar Zbc / vector Zvbc | Map the same carryless products to scalar or vector instructions; retain the portable fallback. | Confirm the extensions, word/vector lengths, toolchain, and actual devices; do not claim accelerated support after cross-compilation alone. |
| Explore after a demonstrated need | Other architectures or newer polynomial/vector crypto extensions | Inspect available polynomial operations and implement the existing field/lane semantics. | A correct backend, executable tests, maintainable toolchain support, and a measured workload win. |

Instruction references: [Arm NEON intrinsics](https://arm-software.github.io/acle/neon_intrinsics/advsimd.html),
[Intel optimization manual](https://cdrdv2-public.intel.com/671488/248966-Software-Optimization-Manual-V1-048.pdf),
[Intel AVX10.2 specification](https://cdrdv2-public.intel.com/828965/361050-intel-avx10.2-spec.pdf),
[RISC-V vector crypto specification](https://docs.riscv.org/reference/isa/extensions/crypto-vector/_attachments/riscv-crypto-spec-vector.pdf).
These describe available instruction families; the table's performance choices
are experiments proposed here.

## Highest-value experiments

### 1. Arrange the algorithm for the actual multiply instructions

The U8 stripe has eight `a` words followed by eight `b` words, with pairs
`(a[i], b[i])`. On NEON, adjacent pairs can use the low and high polynomial
multiplies without first interleaving every input pair. Wider x86 loads can feed
several corresponding pairs. The parity pair is additional work; pack multiple
stripes where that reduces mostly empty vector lanes.

Specify a canonical key order first. A backend may prepare a private packed view
of that key, but measure its extra construction time and memory. Select only the
needed view per prepared key instead of eagerly storing copies for every ISA.
Prove that packing is a permutation of the specified parameters.

Keep `v[0]` through `v[8]` accumulations in registers across a run of stripes.
Perform horizontal combination at the specified chunk boundary. Benchmark one,
two, and four stripes of unrolling, and separate accumulator sets where they
break dependency chains. Reject an unroll that spills or expands instruction
footprint enough to lose in realistic streams.

### 2. Fuse XORs without changing the mathematics

Evaluate Arm EOR3 when its extension is available and x86 ternary-logic XOR for
accumulator updates and parity reduction. Keep a basic XOR path. Reduce needless
cross-lane shuffles before adding more unrolling. Record instruction counts and
register pressure rather than relying on source-level counts.

GFNI or byte-shuffle instructions may help a specific fixed linear transform or
software carryless backend. Use them only after deriving the exact bit mapping.
Byte-field multiplication is not directly a GF(2^128) multiply. Any such path
must match the field oracle and beat the straightforward implementation.

### 3. Fold the outer polynomial while preserving output

The U8/EHC two-coefficient recurrence in [PLAN.md](PLAN.md) is:

```text
P_next = ((P XOR h0) * R XOR h1) * R
```

Precompute `R2 = R * R` in the key. The identical expression is:

```text
P_next = (P XOR h0) * R2 XOR h1 * R
```

This exposes two independent products. Test forming both unreduced polynomial
products, XORing them, and reducing once, because field reduction is linear.
Verify the full 256-bit intermediate representation and reduction explicitly.
`R2` is derived metadata, not additional independent entropy.

For long contiguous input, further folding across chunks with powers of `R2`
may expose more parallel work while preserving message order. Measure first:
the inner compression is expected to dominate, and extra powers, registers, or
buffers can cost more than they save. This optimization is not permission to
change the chunk size or hash a different tree on different CPUs.

### 4. Optimize small messages independently of bulk vector width

Keep short field operations, bounds-checked loads, and output stores compact.
Compare 128-bit and wider helpers, inlined compile-time selection and a cached
dispatch call. Avoid bringing the whole expanded key into cache for a tiny hash.

Backend thresholds may choose different instruction sequences for the same
mathematical branch. The algorithm's 31/32-byte split, padding, key assignments,
and 8 KiB chunk boundaries remain identical everywhere.

### 5. Vectorize key expansion and the optional MAC

Use hardware AES through an appropriately licensed implementation and batch
independent key-derivation blocks. Evaluate AES-NI versus VAES at available
widths, and Arm AES instructions. Preserve the exact labeled input blocks and
derived parameters across backends.

For the approved MAC construction, process its independent AES inputs together.
If an application exposes batches of messages, interleave several independent
tags to improve utilization. Benchmark single-tag latency and batch throughput
separately; batching does not change the nonce policy. Count key expansion and
cipher queries in the security argument as required by the selected derivation.

### 6. Treat memory behavior as an optimization target

The table-based candidates make L1 load bandwidth and cache conflicts worth
measuring. P256 deliberately tests a compact-key alternative. Compare data/key
alignment, unaligned input, load scheduling, and multiple active keys. Resolve
key/state alignment internally rather than forcing callers to realign input.

Try manual prefetch only after counters show a relevant miss/stall pattern; test
several distances and the no-prefetch baseline. Large-buffer experiments must
also cover memory beyond cache. Masked loads still need valid Rust pointer
arithmetic and correct active-lane bounds; validate short tails at guard pages.

## Dispatch, portability, and toolchain rules

Use the pinned toolchain's actual detection/intrinsic contracts:
[Rust x86 detection](https://doc.rust-lang.org/std/arch/macro.is_x86_feature_detected.html)
and [Rust AArch64 detection](https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html).
For example, current Rust documents `sve2-aes` as including the SVE polynomial
crypto capability, while feature-detection support varies by OS. Check this
against the release toolchain rather than assuming all named features work on
every platform.

Compile baseline dispatch code without the optional instructions. Put each
accelerated function behind explicit target-feature preconditions; never execute
it before selection. Use compile-time selection for suitable `no_std` builds.
For scalable-vector backends, do not assume a process-global fixed vector length
if the platform can vary it by thread. Match outputs across tested vector lengths.

Intrinsics are the first implementation choice. Consider isolated assembly only
for a demonstrated code-generation gap, with ABI/clobber review and the same
tests. Feature detection, an intrinsic name, or a successful build does not by
itself establish instruction availability or safe execution on a target machine.

Use measured CPU/workload data to select between equivalent narrow and wide
kernels. Avoid noisy runtime autotuning in the hash call. Add a diagnostic/bench
backend selector that rejects unsupported choices, so each path can be tested
and the selected path can be recorded.

## Acceptance for each optimization

For every proposal, keep a short result record: hypothesis, changed instruction
sequence, output equivalence check, CPU/toolchain, size/update/key workloads,
median and uncertainty, footprint change, and accept/reject decision. Require
the correctness and measurement gates in [BENCHMARKS.md](../BENCHMARKS.md).

Modern-CPU coverage means correct fallback plus validated accelerated paths.
A speed claim is limited to the models and workloads actually measured. Expand
the matrix as new polynomial instructions and tested machines become available;
adding an unused or slower backend is not itself an improvement.

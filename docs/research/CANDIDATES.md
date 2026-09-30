# Candidate portfolio and selection experiments

> Research notes kept for their reasoning. The raw result files they cite (`results/`) were removed before publication; current measurements are in [BENCHMARKS.md](../BENCHMARKS.md).

> **Objective clarified, 2026-09-28: maximum x86 performance with the same
> security or better; changing the algorithm is allowed.** The active replacement
> shortlist and newly derived compositions are in
> [section 9](#9-algorithm-replacement-with-the-same-security-or-better).
> [Section 8](#8-x86-research-update-2026-09-28) contains the implementation
> investigation and broader literature review. This document combines
> inspection of the source and recorded benchmarks with a primary-source
> literature search. No code, tests, or benchmarks were changed or run for this
> update. New speedups below are hypotheses unless explicitly attributed to
> existing measurements or a paper.
>
> **First algorithm experiments:** carryless/integer hybrid compression with
> joint outer hashing; a two-coordinate GF(2^64) chunk compressor; a complete
> SIMD integer compressor. The implementation experiments in section 8 remain
> useful alongside these. The 130–131 GB/s Zen 5 bulk result
> leaves limited arithmetic headroom; it does not establish a limit for short
> messages, other x86 CPUs, or different algorithms.
>
> [ZEN5_CANDIDATES.md](ZEN5_CANDIDATES.md) records valuable earlier probes,
> including losing two-chunk schedules and a +13% hybrid **bare kernel**. Its
> blanket dismissal of all alternatives and its ~1.25× "ceiling" should be read
> with the qualifications in section 8.1. No reviewed alternative has yet been
> demonstrated to beat v1 end to end while satisfying the current contract.
>
> **Measured afterwards (2026-09-28):** [section 10](#10-x86-experiments-2026-09-28)
> records complete, oracle-checked prototypes of the §9.3 hybrid, the §9.5
> integer compressor and the §9.4 GF(2^64) close on the EPYC 9R45, a port
> model of Zen 5, and the X-items tried on v1. The hybrid's loop body reaches
> 38.8 B/cycle (v1's 32), but complete versions, including one fully in asm,
> gain only 7.5% on hot data and 0–4% from L2.
> Adopted implementation changes: a dedicated path for 65 bytes–1 KiB (9–16%
> faster one-shot, MAC 7–12%) and a hardware combine in `hash_parallel`. A
> batch API prototype is 3–4× faster per short message.
>
> **Second round (2026-09-29):** [section 10.7](#107-second-round-2026-09-29).
> Key setup is 35–41% faster, AVX-512 1–31 bytes 1.7–2.2× (4× for random
> lengths), 32–64 bytes 1.25×, 65–256 bytes 1.1×, and bulk 1–3%, all with
> v1's outputs. The progress log is [progress.md](../../progress.md).

Sections 1–6 preserve the original **six-family** portfolio from before the
chain-coded implementation was selected. Their proposed compositions still
require review and formalization; the later Zen 5 screening is recorded in
[ZEN5_CANDIDATES.md](ZEN5_CANDIDATES.md). Section 7 records rejected or deferred
ideas. The 32–64 B specialization was adopted on both ARM and x86; see the
ARM results and
[x86 port](X86_PORT.md).

The fixed requirements remain a 128-bit keyed universal hash, optional MAC,
identical results across CPU backends, and primary NEON/AVX-512 support. The
historical proposals used `(L / 4096 + 3) / 2^128` as their comparison target.
Current v1 has the stronger bound `(ceil(L / 8192) + 1) / 2^128` in
[SPEC.md](../../crates/raijuhash/SPEC.md). A new design must report against
**both**; meeting the old target alone is not equivalent to preserving v1's
guarantee. The current specification governs implementation optimizations;
replacement designs may change outputs, layout, chunking, and key format while
meeting the pointwise security requirements in section 9.1.

## Historical portfolio and original priorities

| Family | Main hypothesis | Most relevant workloads | Main reason it might fail | Priority |
|---|---|---|---|---|
| **EHC: U8 / U16 / U32** | Larger encoding groups amortize parity multiplication. | Reused keys, hot bulk buffers, PMULL/VPCLMUL CPUs. | Parity, register pressure, tails, and key traffic erase the saving. | First: U16 against U8. |
| **M32: Multimixer-based compression** | Ordinary 32-bit SIMD multiplication can use hardware more effectively. | NEON/AVX-512 bulk and CPUs with slower carryless multiplication. | Final reduction, arithmetic carries, and lane movement consume the gain. | First: genuinely different bulk design. |
| **P256: compact-key polynomial hierarchy** | Eliminate a per-position random-key table. | Fresh keys, many active keys, cache-constrained streams. | Wider field arithmetic loses badly on a single hot bulk stream. | First: test the opposite memory tradeoff. |
| **L128: field-linear compression** | Very regular field products and 16-byte streaming units reduce overhead. | Short/medium messages and fragmented updates. | Too many carryless products per byte for peak bulk throughput. | Cheap control and possible short/medium path. |
| **I4: four integer NH streams** | Simple widening integer multiplies can replace almost all per-byte carryless work. | Portable fallback, SIMD without polynomial instructions. | Four key streams create heavy L1 traffic. | Portable control for M32. |
| **D2: two independent carryless NH streams** | Remove parity coding and its shuffles entirely. | CPUs where coding/layout costs dominate. | Sixteen products per 128 bytes instead of ten. | Cheap regular-kernel control; prune quickly if slower. |
| **Pyramid128 (2-level key)** | Derive rows from a small linear basis. | Many keys. | Cancellations invalidate the proposed security reasoning. | Rejected (§7.1). |
| **In-Register Key Rolling** | Derive rows with a simple linear recurrence. | Key traffic. | Does not preserve the independent-key proof. | Rejected (§7.2). |
| **Chain64** | Weaker 64-bit family. | Hash tables. | Outside the required output/bound contract. | Separate project only (§7.3). |
| **SuperChain256** | Interleave more block work. | CPU-specific scheduling. | Dual FMA pipes do not imply dual VPCLMUL pipes. | Lost on Zen 5; other schedules remain experiments (§7.4). |
| **Permuted Chain** | Apply two AES rounds to the digest. | Hypothetical nonce-free MAC. | No adequate PRF/MAC argument. | Rejected (§7.5). |

U8, U16, and U32 are variants of one family, not three unrelated inventions.
Likewise, a different unroll factor, ISA backend, or dispatch threshold is an
implementation experiment rather than another hash algorithm.

## Common proof tools and comparison rules

All proposed random parameters are uniform and independent unless an explicit
theorem permits reuse. Short-seed expansion introduces a separate computational
assumption, as in the original plan.

For an intermediate digest with collision bound `epsilon_inner`, split its
complete fixed-width encoding into 128-bit field elements `Z[j]`. With fresh
independent uniform `A[j]` in `F = GF(2^128)`, consider:

```text
project(Z) = XOR over j of (A[j] * Z[j])
```

Conditioned on distinct intermediate digests, at least one difference coefficient
is nonzero. That coefficient multiplies an independent uniform parameter, giving
an intended AXU bound of `epsilon_inner + 2^-128`. If the intermediate digests
coincide, charge that event to `epsilon_inner`. Include the full digest in the
projection; truncating it is not equivalent. This argument does not assume an
additive-difference bound is already an XOR-difference bound.

For independently keyed chunk compression with collision bound `alpha`, followed
by one field-Horner step per chunk under independent `R`, a fixed different chunk
gives a route to total AXU error `alpha + q / 2^128`, where `q` is the number of
chunks. Reusing the chunk's parameters across chunks does not make chunk outputs
independent; the proof instead conditions on the entire compressed sequence.
Bind the exact byte length with a separate term `length * T`, and prove padding,
branch offsets, and cross-length cases. These are proof templates to check for
each actual byte encoding, not automatic guarantees for arbitrary compositions.

Reuse the specified tiny-message family where appropriate, with independent
branch offsets. A short/medium/bulk combination needs its own complete proof.

## 1. Wider EHC: U8, U16, U32

Generalize the worked [U8 construction](PLAN.md#3-worked-example-u8) to `k`
message pairs plus one parity pair, each with an independent key. Use combination
columns `(1, c[i])`, with distinct field coefficients, plus `(0, 1)` for parity.
Keep the 8 KiB outer chunk and the two-field-element output per chunk.

| Variant | Stripe size | Carryless products per stripe | Products per 128 B | Compression-key table per 8 KiB | Ideal product-count ratio vs current |
|---|---:|---:|---:|---:|---:|
| U8 | 128 B | 9 | 9 | 9,216 B | `10/9 = 1.111` |
| U16 | 256 B | 17 | 8.5 | 8,704 B | `20/17 = 1.176` |
| U32 | 512 B | 33 | 8.25 | 8,448 B | `40/33 = 1.212` |

Those historical ratios concern inner multiplication count only. They exclude
all key loads, parity, combination, padding, and outer arithmetic. This family
approaches eight products per 128 bytes, a `1.25` count ratio against the old
ten-product baseline. Current chain coding already uses 8.125 per 128 B for a
full chunk; these ratios are not prospective gains over v1.

**New engineering choices:** compare storing one accumulator per encoded symbol
with accumulating coefficient bit planes. For `c[i] = i`, each coefficient is a
short binary polynomial; sums for each coefficient bit can reconstruct the
combination with field shifts. This saves accumulator space at the price of
more XOR work. Larger stripes also increase the pending-tail requirement.
U32 must not inherit U8's 128-byte tail or 512-byte state estimate.

**Proof work:** generalize the distance-two and invertible-column arguments;
verify independent keys at every stripe position, and preserve the existing
two-coefficient outer bound. Derive the new tail/layout specification explicitly.

**First experiment:** U8/U16/U32 compression-only kernels on NEON and AVX-512,
then end-to-end 256 B, 4 KiB, 64 KiB, irregular updates, and multiple keys. Reject
a wider group when its measured total cost loses. Do not let a CPU select a
different group size under the same algorithm identifier.

Public mathematical basis: [Nandi's EHC construction](https://eprint.iacr.org/2013/574).

## 2. M32: integer SIMD Multimixer composition

**Superseded composition:** section 9.2 uses the paper's tighter equal-length
bound and combines the complete digest with the outer polynomial. The older
conservative accounting below is retained as research history; it is not an
unavoidable security penalty of an M32-based replacement.

Multimixer-128 is a published keyed compression construction using 32-bit integer
multiplication, with a stated universality bound of `2^-127`. Its key-then-hash
structure is a distinct alternative to carryless EHC.
[Original paper](https://eprint.iacr.org/2023/1357).

**Candidate:** implement its mathematical compressor independently, preserving
its full specified intermediate digest and key requirements. Use independent
field projection as above to target a 128-bit AXU result. The conservative
proposed bound is `3 / 2^128`: `2^-127` for an internal collision plus `2^-128`
for projection. Do not interpret the algorithm's name as its digest width or
silently truncate the internal digest.

For bounded messages, evaluate this compressor directly without outer chaining.
For long streams, reuse a fixed **8 KiB** compression-key table by chunk, project
each chunk, and apply the one-coefficient outer family. A possible composition is:

- Up to 31 bytes: the common short family.
- 32–8,192 bytes: direct compression, projection, independent length binding.
- Above 8,192 bytes: `q` projected chunks and an outer bound `(q + 3) / 2^128`.

For the last case, `q = ceil(L / 8192) <= L / 4096` when `L > 8192`, so the
proposed bound fits the target envelope. The direct medium branch avoids paying
an extra outer error term on a one-chunk input. Independent branch offsets and
cross-length treatment are part of the specification, not optional decoration.

**Why it might win:** widening integer multiply instructions provide a different
throughput/latency balance from polynomial multiplication. The body can use
NEON widening products and AVX2/AVX-512 widening products, with field arithmetic
amortized at chunk boundaries.

**Why it might lose:** the published construction's complete additions, carries,
encoding, and digest folding all count. An integer multiply count alone is not
a fair comparison with one carryless multiply. Projection may dominate medium
messages, and key traffic remains proportional to the chunk.

**First experiment:** extract exact operations and parameter sizes from the
paper, write a scalar oracle, then measure the entire compressor plus projection
on both target ISAs. Verify the theorem applies to the chosen block count and
padding before accepting the composition. This is the first alternative to
investigate alongside U16, rather than assuming carryless EHC must win.

## 3. P256: compact-key polynomial hierarchy

**Candidate:** use a polynomial family over `GF(2^256)` internally, then reduce
to 128 bits with an independent field-linear projection of both 128-bit halves.
This trades more arithmetic for far less expanded key material.

Start with a precise control construction: zero-pad to 32-byte elements, apply
Horner under uniform `R256`, and bind byte length with an independent uniform
`T256`. For two unequal messages of the same length, the polynomial root argument
targets at most `q / 2^256`, where `q = ceil(length / 32)`. Different lengths
are handled by `T256`. Final projection then targets:

```text
epsilon <= q / 2^256 + 2^-128
```

With the `u64` byte-length limit, `q <= 2^59`. This offers ample margin against
the old envelope if the encoding and proof check out. The core mathematical key
for this control can be 112 bytes: two 256-bit parameters, two 128-bit projection
parameters, and a 128-bit offset. Common short keys, cipher schedules, and cached
powers are additional and must be included in footprint measurements.

Then test a different polynomial variant using fixed-size **BRW leaves and a
Horner outer level**, with independently specified keys and a recomputed degree
bound. Published work implements this general hierarchy over both 128-bit and
256-bit fields. BRW reduces the number of field multiplications compared with
straight Horner, while squaring, reduction, and buffering still contribute.
[Chakraborty, Ghosh, and Sarkar](https://eprint.iacr.org/2016/1103).

**Why it might win:** fresh-key cost, many active connections, and cache pressure
can dominate arithmetic. This candidate avoids reading a 4–9 KiB table per
streaming chunk and can keep most parameters in a small cache footprint.

**Why it might lose:** a 256-bit field product is substantially more work than a
64-bit carryless product. Ordinary Karatsuba decomposition uses nine 64-bit
carryless products before reduction; it does not promise a hot-buffer throughput
win. Measure powers, leaf stacks, reduction, and projection too.

**First experiment:** validate a field modulus and independent scalar model,
measure GF(2^256) operations, then compare setup-inclusive cost and 16/256/4096
concurrent keys. Prune if it loses its intended workload even with compact state.

**Research extension:** inspect the injective hashing construction in
[Ahle–Knudsen's polynomial work](https://arxiv.org/abs/2609.06022) as another
schedule/family choice. Do not apply a characteristic-zero theorem blindly to
binary fields, or count preprocessing of message-dependent coefficients as free.
Changing the polynomial family changes the digest and requires a new proof.

## 4. L128: independently keyed field-linear chunks

For a 4 KiB chunk, load 256 field elements `M[i]`, zero-padding its final 16-byte
element. Draw independent uniform field keys `K[i]` and compute:

```text
C(chunk) = XOR over i of (K[i] * M[i])
P_next = (P XOR C(chunk)) * R
H(message) = P XOR (length * T) XOR S
```

For equal-length inputs, a different element has a nonzero coefficient of an
independent key. This targets `2^-128` chunk AXU and `(q + 1) / 2^128` after
the outer composition, fitting the old envelope with `q = ceil(L / 4096)`.
Prove the different-length and short-mode cases separately.

**Why it might win:** 16-byte buffering, a tiny number of running accumulators,
regular loads, no parity, and no code-combination matrix. It is also a useful
independent control for tiny/medium-message field operations.

**How to implement efficiently:** XOR unreduced 256-bit products for a chunk and
reduce once. With three-product Karatsuba, the body still needs 24 carryless
64-bit products per 128 input bytes, compared with ten currently. This is a
credible latency/state experiment, not the leading bulk-throughput hypothesis.

**First experiment:** 0–512-byte one-shot calls, 1/7/16/31-byte update patterns,
and state size. Test its specialized short/medium branch within another candidate
only with independent domain parameters and a complete combined proof.

## 5. I4: four independent 32-bit NH streams

NH's ordinary integer construction uses additions modulo `2^32`, widening
32 × 32 products, and accumulation modulo `2^64`. Its equal-length collision
bound supplies a public basis for this experiment.
[UMAC specification, NH section](https://www.rfc-editor.org/rfc/rfc4418).

Run four NH instances with independent keys on each 4 KiB chunk. Concatenate
their four 64-bit results, then project the resulting 256 bits with independent
128-bit field parameters. Four independent `2^-32` collision bounds give an
intended joint `2^-128` collision bound; projection targets `2 / 2^128` AXU.
One outer Horner step per chunk yields the proposed `(q + 2) / 2^128` envelope,
which fits `L / 4096 + 3` when `q = ceil(L / 4096)`.

**Why it might win:** ordinary widening multiplication works without PMULL or
PCLMUL. Only the chunk projection/outer level needs emulated field arithmetic on
such a machine, rather than every input pair. The loops are suitable for NEON,
AVX2, and AVX-512 integer SIMD.

**Cost to expose:** 64 widening 32-bit products per 128 input bytes, and **16 KiB
of compression keys** for four independent streams over 4 KiB. This can lose on
load bandwidth and many-key workloads even when integer multiplication is fast.

**First experiment:** scalar and SIMD multiplication kernels on hardware without
fast carryless multiply, then compare against the portable U8 backend and M32.
Use four genuinely independent key sets; a few tweaks to one NH output do not
provide the product of collision probabilities.

**Later variant:** use integer encode-hash-combine to reduce the repeated work.
[HalftimeHash](https://arxiv.org/abs/2104.08865) provides relevant research, but
the ring's noninvertible matrices can weaken bounds. Account for that loss and
the final projection explicitly; do not label its smallest 128-bit output as
automatically providing the required 128-bit AXU guarantee.

## 6. D2: two independent carryless NH streams

Use two independent key tables. In each stream, XOR-accumulate a carryless
product of each keyed 64-bit pair. Each stream targets `2^-64` AXU; independent
keys target a joint `2^-128` bound on the two 128-bit results. Feed that pair into
the same two-coefficient, 8 KiB outer construction as U8.

**Why it might win:** no parity calculation and no EHC coefficient transform;
the regular independent products may schedule well. It is a useful test of
whether the proposed wider EHC kernels actually repay their coding overhead.

**Why it probably loses on hot bulk data:** 16 carryless products per 128 bytes
and 16 KiB of keys per 8 KiB chunk. That is substantially more multiplication
and key traffic than U8. Keep this experiment small and discard it promptly if
the expected loss appears.

**Later variant:** investigate a published Toeplitz key-sharing amplification
construction to reduce key footprint. Reusing shifted keys removes the easy
independence argument; it is a new proof obligation, not a free optimization.
[Nandi discusses Toeplitz and independent-copy constructions](https://eprint.iacr.org/2013/574).

## 7. Next-generation candidates (post-raijuhash exploration)

These are dispositions of earlier proposals, not approved speed or security
claims. Their original numerical forecasts were not measurements.

### 7.1 Pyramid128: 2-level hierarchical tensor keying

**Rejected.** Generating rows as `K[8u+v] = Krow[v] XOR u*Kcol` introduces
linear dependencies and cancellation opportunities. The independent-key proof
does not survive. Use canonical AES expansion or a separately proved compact-key
family instead. The former forecast of a fivefold setup improvement is withdrawn.

### 7.2 In-register key rolling (zero-memory key streaming)

**Rejected in this form.** Rotating/XORing a seed into successive rows does not
give the required independent parameters. This does not reject every published
LFSR or Toeplitz universal family, each of which has its own theorem. Regenerating
the exact AES counters of a seeded v1 key remains an output-preserving storage
experiment (§8.2, X11); it is computationally expensive, not free key traffic.

### 7.3 Chain64: single-lane 64-bit table hash

**Outside scope.** A weaker, separately specified 64-bit family may be useful,
but it cannot fulfill this project's 128-bit contract. Removing half the current
lanes would omit input words unless the layout is redesigned. Neither the old
1.5–2 ns prediction nor its proposed bound was established here. Universality
alone also does not establish resistance to adaptive hash disclosure.

### 7.4 SuperChain256: 256-byte dual-block kernel for AVX-512

**Rejected hardware premise; scheduling remains CPU dependent.** Zen 5's dual
FMA capability does not imply two VPCLMUL instructions per cycle. The recorded
two-chunk schedules lost, and the 150–160 GB/s forecast for an unchanged v1
compression kernel is unsupported. A 256-byte loop unroll, two whole chunks in
lockstep, and overlapping only a chunk epilogue are distinct experiments. The
negative result for one does not prove that all three lose on every x86 CPU.

### 7.5 Permuted Chain: nonce-free universal PRF / MAC

**Rejected.** Two AES rounds on a universal-hash output do not provide the
claimed PRF or nonce-free MAC guarantee. The current nEHtM construction remains
the specified MAC. Published AES-based designs are useful research comparisons
(§8.3, R6), with their actual assumptions and full finalization costs.

## Ideas to apply across candidates

This is the original cross-candidate list; several items are now implemented.
Section 8 identifies the remaining experiments and their evidence.

- **Lazy or on-demand parameter expansion:** derive only the needed prefix or
  stream the canonical key schedule with AES/VAES. Compare caching against
  recomputation. This may help fresh keys and hurt long streams; keep derivation
  domains and resulting parameters identical. It is a key-storage strategy,
  not a seventh universal-hash family.
- **Hybrid short/medium/bulk families:** use the field-linear short path and the
  selected long family with independent offsets/parameters. Prove the complete
  cross-branch AXU bound. CPU-specific instruction selection must still produce
  identical bytes for every branch.
- **Chunk folding and message batching:** expose independent polynomial products
  or hash multiple messages in SIMD lanes. Preserve per-message output and nonce
  semantics; report single-message latency separately from batch throughput.
- **Many-core composition:** consider parallel chunk evaluation only after the
  single-core comparison. Ordered polynomial combination can be parallelized
  with the correct powers and chunk counts. A different tree layout is a new
  algorithm, not an invisible threading optimization.

## Historical selection process and stopping rules

The proof and comparison principles below remain useful. Section 9.8 supersedes
the original family-priority order for the next x86 implementation round.

1. **Proof screening:** write a complete parameter/encoding sheet and symbolic
   bound for every family. Reject any that misses the required envelope or MAC
   assumptions. Distinguish arithmetic AXU, additive universality, and collision
   bounds throughout.
2. **Small prototypes:** test U16, M32, and P256 first. Implement L128, I4, and D2
   only as bounded controls for their stated hypotheses. Use scalar oracles and
   tiny kernel comparisons before creating full crate APIs.
3. **Compare real costs:** record arithmetic, loads, shuffles, reductions, key
   expansion, prepared-key size, stream state, and end-to-end time. Test at least
   NEON/PMULL and Intel/AMD AVX-512 as in [BENCHMARKS.md](../BENCHMARKS.md).
4. **Keep the useful tradeoffs:** retain candidates that win a target workload
   without breaking the contract. Stop work on dominated designs. U32's state
   costs and P256's arithmetic costs belong in the decision table.
5. **Prove and finish survivors:** formalize the selected complete construction,
   validate its optional MAC, and implement the production backends. Each
   alternative needs a proof for its own construction.
6. **Freeze one cross-platform specification:** do not silently run M32 on Arm
   and U16 on x86 under one hash identifier. If real users need two winning
   tradeoffs, expose separately named/versioned algorithms with explicit choice.

For each experiment record: candidate/version, hypothesis, theorem status,
hardware, input/update/key distribution, measured result, and keep/reject reason.
The old single-candidate plan is therefore replaced by a selection process,
with U8 retained as the most detailed worked example.

Published algorithms mentioned here are sources of mathematical ideas and
comparison points. Independent implementation and any use of existing code must
follow [PROVENANCE.md](PROVENANCE.md); this document does not assign those works
a new license.

## 8. x86 research update, 2026-09-28

This is a broad survey of relevant arithmetic, hashing, MAC, and processor
research, including work from 2025–2026 and independent deductions from v1's
source. It cannot establish that every relevant paper or possible optimization
has been found. Sources and access limits are recorded in §8.5. No new x86
measurements were made for this update.

### 8.1 What the measurements establish, and what remains open

The baseline is the optimized EPYC 9R45 report, its raw outputs,
and [X86_PORT.md](X86_PORT.md). The source reviewed includes
[AVX-512](../../crates/raijuhash/src/x86/avx512.rs),
[SSE/AVX2](../../crates/raijuhash/src/x86/mod.rs),
[AES](../../crates/raijuhash/src/x86/aes.rs),
[API/key handling](../../crates/raijuhash/src/lib.rs), `state.rs`, `mac.rs`,
and the comparison harness. The older `PLAN.md` and `SIMD.md` describe U8 in places: their derived
`R2 = R*R` must **not** be substituted for v1's **independent** parameter `R2`.

| Recorded observation | Consequence for the next experiment |
|---|---|
| 64 KiB: about 130 GB/s. | Hot bulk arithmetic is already efficient. Focus on residual overhead, not a predicted doubling from unrolling. |
| 16 MiB: about 102.5 GB/s; memory-sized inputs: 42–53 GB/s in the later study. | Cache and memory delivery matter. Compute-only gains will shrink here. |
| 16 B: 7.0 ns streamed / 5.4 ns one-shot; 31 B: 7.7 / 6.0 ns; 64 B: 6.3 / 4.5 ns. | Smaller inputs are not always cheaper: short-path field arithmetic and API overhead merit work. |
| 256 B: 10.7 / 8.4 ns streamed / one-shot. | There is measurable overhead outside the compressor. |
| Setup: 100 ns; key: 9344 B; stream: 1536 B. | Setup is **lazy**, generating nine rows plus field parameters, not the complete table. Time first long use too. |
| Forced AVX2/SSE: about 66/34 GB/s at 64 KiB on Zen 5. | These are width comparisons on one CPU; they do not predict dedicated AVX2-only hardware. |

**A more precise ceiling.** A full chunk has `65 * 8 = 520` carryless
64×64 products, including its endpoint. Four products fit a zmm instruction;
the measured issue interval is two cycles. This gives `130 * 2 = 260` cycles
per 8192 bytes, or **31.51 B/cycle ≈ 141.5 GB/s at 4.49 GHz**, before field
finalization and all other overhead. The 144 GB/s figure omits the endpoint.
Relative to 130 GB/s, the tighter inner-product limit leaves about **8.8%**,
and the attainable whole-hash gain is smaller if this instruction count stays
fixed. These are calculations from the recorded issue rate, not measurements.

The `10/8 ≈ 1.25` comparison is a ratio of idealized body costs. It is neither
a bound on measured speedup over an implementation with overhead nor a lower
bound for every universal-hash construction. [Nandi's lower bound][nandi] has
an algebraic computation model; it is not a theorem about x86 instruction
throughput. A better schedule can preserve a digest, and a different algebraic
evaluation need not automatically mean a new algorithm.

Also qualify the earlier screening:

- Its Multimixer result is explicitly a **Multimixer-shaped** kernel. Its
  HalftimeHash numbers measure a particular Rust crate/build. They are useful
  negative evidence, not optimality proofs for the published families.
- The +13% hybrid result is a scratch inner-loop result, not a full hash and
  not a ceiling on all mixtures. Recover the exact prototype before treating
  it as reproducible evidence.
- Poly1305 uses integer arithmetic. A carryless-product floor for GHASH or
  binary-field BRW does not apply to Poly1305 or IFMA implementations.
- P256's earlier `≥44` products per 128 B is not a bound on every polynomial
  schedule. For example, a BRW body approaches two GF(2^256) multiplications
  per 128 B, or 18 base products with nine-product Karatsuba, **before** its
  reductions, squarings, and outer work. This still looks costly on Zen 5.
- A loss for two complete chunks in lockstep does not test every within-chunk
  unroll or partial overlap of an epilogue with the next chunk.

### 8.2 Candidates that preserve the v1 digest

Priority denotes experimental value, not confidence in a measured speedup.
All items are proposals unless identified as already implemented. Changes to
state layout or API may be needed even when mathematical outputs stay identical.

| ID | Experiment | Main workload | Priority |
|---|---|---|---|
| X1 | Pack the partial products of short field multiplications into SIMD lanes | 1–31 B, avalanche, parallel combine | High |
| X2 | Three-product Karatsuba with prepared key cross-sums | Short/medium finalization, SSE/AVX2 | High |
| X3 | Select reduction and lane-fold order for each CPU/path | Finalization and chunk close | High |
| X4 | Specialize zero halves and public length contributions | 1–8 B, exactly 16 B, 32–64 B | Medium |
| X5 | Evaluate a partial quadratic form for 65–128 B | Packet tails, small records | Medium; algebra first |
| X6 | Reduce copies and repeated chunk-state folding | Fragmented streams, 1–8 KiB | High |
| X7 | Bring structural optimizations to SSE/AVX2 | Zen 3, Intel client CPUs, older PCLMUL CPUs | High |
| X8 | Select vector width and scheduling by measured CPU behavior | Intel AVX-512, hybrid CPUs, Zen 4/5 | High |
| X9 | Overlap chunk close with the next chunk's prefix | Hot bulk on one core | Medium |
| X10 | Expand fewer key rows; improve AES batch sizes and setup scheduling | Fresh keys, short-lived keys | High |
| X11 | Offer compact storage of canonical seeded parameters | Many active keys | Medium; workload specific |
| X12 | Overlap nonce-only AES work with hashing | Existing MAC | Medium |
| X13 | Batch independent short hashes/MACs | Packet or record batches | High when batching exists |
| X14 | Accelerate parallel combination and reuse workers | Large buffers on several cores | High for parallel API |
| X15 | Tune memory delivery and fuse existing input passes | Unaligned buffers, L3/DRAM, read-and-hash | High when memory limited |
| X16 | Synthesize fixed linear transforms with GFNI/shuffles | Reduction, many small field operations | Low; instruction-count screen first |
| X17 | Audit generated code, dispatch, and instruction footprint | Generic binaries, mixed message lengths | Medium |
| X18 | Vectorize software carryless multiplication | x86 without usable PCLMUL | Low overall; useful fallback target |

#### X1. Fill SIMD lanes with partial products of one short hash

`avx512::short` delegates to `x86::short`, which uses `Wide::mul` with four
**xmm** carryless instructions per full field product, then two for reduction.
The general one/two-product source paths therefore contain six/ten
scalar-width CLMULs, before any compiler specialization.
This is a concrete gap, not a request to reimplement the adopted 32–64 B path.

Independently derived packing experiment: for `a = a0 + a1*x^64` and
`k = k0 + k1*x^64`, put `(a0,a1,a0,a1)` in the selected 64-bit halves of four
128-bit lanes and pair them with `(k0,k1,k1,k0)`. **One zmm CLMUL** produces
the four schoolbook partial products. Reassemble `lo`, `mid`, `hi` with XORs
and shuffles, then reduce. Two vectors cover the two field products at 16–31 B.
Alternatively pack three Karatsuba products, or pack independent complete field
products and share their instruction sequence. Compare the packing costs.

Prepared keys can store the required permutations. Measure direct masked byte
loads versus `partial16`, keeping the exact length marker at byte 15 of the last
field element. The SIMD instruction-count reduction is real algebraically;
the speedup depends on input construction, shuffles, and the reduction chain.
Apply the winning field helper to avalanche and X14 as well. On narrower CPUs,
try ymm packing or the existing xmm schedule. [CLMUL arithmetic reference][clmul].

#### X2. Karatsuba where whole field products are still schoolbook

Both `Wide::mul` and AVX-512 `mix` use four base products per field product.
Prepare `k0 XOR k1`; compute

```text
lo  = clmul(a0, k0)
hi  = clmul(a1, k1)
mid = clmul(a0 XOR a1, k0 XOR k1) XOR lo XOR hi
```

This saves one multiply at the price of operand preparation. Benchmark it in
`mix`, `mul_key`, short hashes, and SSE/AVX2 outer steps; retain specialized
64×128 multiplication for the length term. Intel's [GCM optimization paper][gcm]
explicitly explains why three-product Karatsuba and four-product schoolbook
can trade places as CPU costs change. Saving one zmm issue in a bulk chunk is
only two cycles out of hundreds on Zen 5; short finalization is the better
initial target. The eight independent 64×64 products in `position` are not a
single 128×128 multiplication to which this saving applies.

#### X3. Reduction, horizontal folding, and internal representation

AVX-512 already uses shift reduction in bulk and two-CLMUL reduction in smaller
paths. SSE/AVX2 `Wide::reduce` uses the latter everywhere. Compare shift/XOR,
multiply-based, and derived Montgomery/Barrett schedules **per path and CPU**.
Test XORing unreduced lane results before one narrow reduction against reducing
in lanes then folding; reduction is linear, but shuffle dependencies differ.
Inspect spills, critical-path length, and resource contention, not just CLMULs.

[Gueron–Kounavis][gk] and [Intel's CLMUL paper][clmul] are the starting points.
A reflected or Montgomery representation can preserve v1 only with exact
conversions of inputs, keys, state, and outputs. [RFC 8452's GHASH/POLYVAL
relationship][polyval] illustrates the required bit-order care; changing the
field polynomial alone changes the hash. This is a small helper experiment
before considering a complete internal representation change.

#### X4. Exploit public length and zero halves

For `L <= 8`, the message portion of `X0` occupies at most 64 bits; its
`L << 120` marker can be multiplied separately or prepared for each public
length. Exactly 16 B has an all-marker `X1`. In 32–64 B, `L*T` is also a
small public-length contribution. Try computing or caching these terms and
skipping mathematically zero half-products. A 32-entry u128 marker table costs
512 B per key, so start with a few common lengths or a small basis of marker
powers rather than assuming the table pays for itself.

Branch only on public lengths; preserve all marker/padding semantics. Count
setup and cache costs. Precompute final short coefficients instead of repeatedly
transforming `A`, `B`, or `T` at each call where X1/X2 need a special layout.

#### X5. Extend small-message algebra without pretending it stays affine

The adopted S2 shortcut works because the Y row is zero for 32–64 B. For one
block of 65–128 B, write its word pairs as `(X[l],Y[l])` and key rows as
`(a0,b0)`, `(a1,b1)`. Direct expansion of SPEC §4 gives

```text
H = C + L*T + sum_l (X[l]*Ex[l] + Y[l]*Ey[l]
                     + clmul(X[l],Y[l])*R2)
Ex[l] = b0[l]*R + b1[l]*(R + R2)
Ey[l] = a0[l]*R + a1[l]*(R + R2)
```

Here unqualified `*` is the specified GF(2^128) product, with 64-bit words
embedded in the field; `C` contains the key-only terms and `S`. At 65–72 B only
one Y word can be nonzero. Screen partial-word variants with prepared `Ex/Ey`,
sharing the `R2` multiplication across the sum of quadratic products. At 128 B
the linear terms may make this more expensive than the current two-position
kernel. This is an algebraic candidate, not a blanket extension of S2 or a
predicted win; discard after operation counting if dominated.

#### X6. Streaming work outside the compressor

The one-shot/streamed gap and current 1 KiB pending buffer justify measuring
copying, `update`, state folding, and `finalize` independently. Try direct
processing of complete groups with a cheaper representation of pending work,
or a small initial mode that promotes to full streaming state only when needed.
Preserve ownership: `update` cannot keep a pointer to caller-owned bytes after
returning. Delaying work requires storing bytes or sufficient hash state.

Compare retaining folded `(h0,h1)` with retaining selected lane sums over many
small updates; the former is already implemented and minimizes state stores,
whereas the latter may avoid repeated horizontal folds. Tune 128 B versus
1 KiB buffering only with fragmentation measurements. A new consuming
`update-and-finalize` convenience path or `hash_slices` API can avoid a final
copy when all slices remain available during the call. Keep one-shot and
incremental benchmarks distinct, and do not count API substitution as a faster
streaming implementation. State-size reduction also matters for many streams.

#### X7. Dedicated AVX2/SSE work is still worthwhile

The generic backends retain eleven position/plane sums, stack padding, and
scalar field finalization. Evaluate the AVX-512 structural ideas at their own
widths: direct final partial chunks, compact streaming sums, a fresh-chunk
specialization, batched field products, and lower-overhead endpoints. SSE/AVX2
need safe narrow loads or a small padded tail rather than AVX-512 byte masks.

On AVX-capable CPUs with PCLMUL but without VPCLMUL, test VEX-encoded 128-bit
PCLMUL with AVX2 loads/XORs. Avoid repeatedly extracting every product from ymm
registers if parallel xmm chains schedule better. Check emitted encodings;
AVX2 alone does not imply 256-bit carryless multiplication. The test-only
`avx2emu` backend is a structural control, not evidence of an optimal hybrid.
Use Haswell/Skylake or Zen 2 for that case, and Zen 3/Intel AVX2+VPCLMUL for
the genuine wide-multiply case.

#### X8. Processor-specific widths and schedules

The current widest-supported dispatch is a starting point. Test xmm, VEX ymm,
EVEX ymm, and zmm as applicable. EVEX at 256 bits can retain masks, ternary
logic, and more registers without 512-bit data operations. For example,
[uops.info's measured ymm VPCLMUL results][uops-vpclmul] differ between
Ice Lake (2 cycles/instruction), Alder Lake P (1), Arrow Lake P (0.5), and
Zen 3/4/5 (2). These are **ymm** measurements, not inferred zmm rates or whole
hash speeds. Operand latencies can differ too.

Measure Intel server generations and P/E cores separately. Record wall time
and effective frequency in both sustained hashing and intermittent calls mixed
with scalar application work. Intel documents [frequency effects for crypto
instructions][intel-crypto]; they are model/workload dependent. Do not inherit
the Zen 5 prefetch, reduction, or unroll choices automatically. Require the
exact CPUID/OS-supported features, including optional GFNI, IFMA, or VBMI2,
rather than assuming a CPU family name implies them. [EVEX vector lengths][vl].

#### X9. Hide chunk-close bubbles with limited lookahead

Try starting only the first one or two positions of chunk `i+1` while the
shifts/folds/reduction of chunk `i` complete. The next compressor starts from
zero and does not need the prior outer value until its close. This preserves
the one-chunk inner-loop organization and needs fewer live sums than two entire
chunks in lockstep. Limit lookahead if registers or instruction footprint grow.

For a separate fold experiment, v1's exact recurrence is

```text
C_i = h0_i*R + h1_i*R2
P_(i+k) = P_i*R^k + sum_(j=1..k) C_(i+j)*R^(k-j)
```

Use this identity for ordered chunk batches with prepared powers of **R**.
`R2` remains independent. It exposes parallel field work but can add products
and key traffic; long chunks already hide much of the dependency. Compare
two/four-chunk folds only after X2/X3, with the earlier negative result as a
baseline. This is not a proposal to change the 8192-byte chunk definition.

#### X10. Key setup: less eager work and better AES utilization

Nine eager rows are 72 AES counter blocks, although a 1–31 B hash only needs
field parameters and a 32–64 B hash needs two rows for its prepared form.
Consider a compact initial key mode, deferring rows and S2 coefficients until
needed, with a measured break-even against the new branch/synchronization cost.
`absorb` currently completes the entire table even for some partial-chunk
streaming workloads; request only the needed row prefix where the invariants
permit it. The one-shot partial-chunk path already does this.

`ctr_wide` processes 72 blocks with 18 vectors, then falls back to batches of
eight/four blocks. Sweep 8/12/16/18 vector states and efficient medium remainders,
round-key broadcasts versus loads, and 256-bit VAES on CPUs lacking usable
512-bit VAES. Generate independent field counters alongside row counters if
packing/storing them saves startup. Consider retaining the AES round schedule
across lazy expansions: another 176 bytes trades memory for reconstruction time.
The existing AES-NI schedule was already shortened; measure before replacing it.
[AES instruction reference][aesni].

#### X11. Compact seeded keys and canonical regeneration

For hundreds or thousands of active keys, the 9344-byte object can dominate
cache capacity even though expansion is lazy. Evaluate storing the seed,
prepared short coefficients, and optional separately allocated rows; keep a
no-allocation variant for `no_std`. Avoid requiring the largest table layout for
every short-lived key. Allocation, first use, cloning, and synchronization count.

An alternative is regenerating **the exact canonical AES counters** a chunk
needs, or caching a subset of rows. This preserves seeded v1 outputs but cannot
compress arbitrary `from_entropy` keys. Ten AES rounds per generated 16-byte
block make continuous regeneration a substantial cost; check a kernel budget
before implementing it. Target key-cache thrashing, not an assumed improvement
over an already resident Zen 5 key. This is distinct from rejected linear rolling.

#### X12. Overlap the existing MAC's independent AES input

`Mac::tag` currently hashes first, then calls `encrypt2_xor`. Its first AES
input `nonce & !TOP` is already known before hashing; only the second depends
on the digest. Try scheduling the first encryption alongside short/medium hash
work, or precomputing it when a protocol already knows the nonce. Fuse the
one-shot routine only if dispatch and register pressure stay reasonable.

Also compare two independent AES-NI chains with one 256-bit VAES chain.
They have different issue/packing costs; one tag may be latency limited even
when batches gain greatly. Preserve the two AES inputs, full round count,
domains, and final XOR. This is scheduling within nEHtM, not a new MAC mode.

#### X13. SIMD across independent messages

A `hash_many`/`tag_many` experiment can fill lanes with separate 1–64 B
messages, amortize dispatch, and keep AES chains busy. Group by public length
or use masks; include pointer loads, packing, output scatter, and queueing delay.
Same-key batches can share prepared coefficients, while mixed-key batches need
their own coefficient traffic. Preserve message order and each MAC's nonce.

For long inputs already saturating multiplication, do not expect greater
compute throughput from batching. For fragmented network packets, scatter/gather
input and interleaving ready streams may reduce stalls without copying packets
into a contiguous buffer. Report hashes/s and amortized ns/message separately
from the latency of an isolated call.

#### X14. Parallel hashing has avoidable serial work

`hash_parallel` currently creates scoped threads on every call, allocates a
`Vec<Mutex<u128>>`, distributes 128 KiB tasks with one shared atomic, and
combines results with **`portable::gf_mul`**; `gf_pow` also uses portable
arithmetic. Try hardware field helpers, cached powers for common task sizes,
an existing/persistent worker pool, and independently owned result slots.
Coarsen tasks or assign contiguous ranges if atomic scheduling and result-cache
traffic dominate. A balanced ordered affine composition can reduce combine
depth: represent a task as `(R^m, C)` and compose in message order.

The earlier 8-thread gain at 16 MiB is real reported evidence; thread creation
explains part of the poor crossover. Memory bandwidth still limits large
streams. Pin workers/allocate locally only in experiments that control topology;
report core counts and all-core clock separately from single-core speed.

#### X15. Alignment, prefetch, and avoiding extra memory passes

The harness chooses one favorable aligned address. Test offsets 0–63, page
boundaries, rotating buffers, and many key addresses. Split cache-line loads
can favor loading aligned blocks plus reconstruction or narrower vectors;
do not realign by skipping bytes, since block positions are specified. Any
reconstruction must include prologue/epilogue and stay within valid input memory.

Current prefetch is gated by **the chunk count of one `chunks` call**. Thus a
large message in small updates may get a different policy from one large call.
Try a smaller number of hints per group, adaptive call-size thresholds informed
by cache capacity, and no software prefetch; the losing beyond-L3 configuration
must remain in the comparison. Use huge pages/NUMA placement only as explicit
application or benchmark options when TLB or remote-memory counters justify them.

If an application already copies, parses, encrypts, or reads the input, a fused
copy-and-hash or producer/consumer pass may save a second read. This can improve
end-to-end throughput without making the isolated hash faster. Non-temporal
stores are an option for a genuinely streaming copy destination, not for the
hasher buffer that is immediately read. Ordinary write-back memory does not
acquire magic streaming-load bandwidth from using `MOVNTDQA`.

#### X16. GFNI for fixed linear work, not a magical wide multiplier

GF(2^128) reduction and multiplication by a prepared constant are binary linear
maps. [GFNI affine operations][gfni] can implement 8×8 pieces; byte shuffles
and XORs assemble wider maps. Screen the small overflow correction in reduction
and fixed bit transforms first. For example, the `0x87` correction spills across
byte boundaries, so one byte-local affine instruction is not the whole result.
Count all cross-byte operations and compare with the existing shifts.

An arbitrary full field multiplier needs many byte maps. It is unlikely to beat
X1/X2 for one field element, but transposed batches may amortize permutations.
Use register shuffles or fixed-access circuits, not message/secret-indexed
memory tables in the MAC path. Constructing GF(2^128) as a GF(2^8) tower is a
separate representation experiment requiring verified isomorphisms.

#### X17. Compiler, dispatch, and front-end experiments

Inspect release assembly for spills, unnecessary stack initialization, key
reloads, redundant masks, repeated dispatch, legacy-SSE/AVX transitions,
`vzeroupper`, and excessive function cloning. The source intentionally uses
`MaybeUninit` and out-of-line long paths already; verify their actual effect
in both generic runtime-dispatched binaries and native builds. Compare measured
inlining and modest unroll variants, LTO/codegen settings, and profile-guided
layout trained on mixed lengths. Isolated assembly is justified by a specific
code-generation defect, not assumed superiority.

The reported 4 KiB aliasing stall deserves a randomized stack/key/buffer layout
distribution, not a single favorable process layout. Alignment can reduce split
loads yet create cache-set or false-dependency patterns. Benchmark changes must
not silently erase this tail behavior. Keep instruction bytes and front-end
counters alongside the fastest warm-loop time. [AMD guide][amd-sog],
[Intel guide][intel-opt].

#### X18. Faster fallback without changing the function

`portable::clmul32` already splits bits into four classes with gaps that absorb
integer carries, and `clmul64`/`gf_mul` already apply Karatsuba. Do not count
introducing either technique as new. On x86 with SIMD but no usable PCLMUL,
compare vectorizing independent integer partial products with a Boolean
bitsliced multiply, measuring transpose/setup costs and the full hash.
[BearSSL's original arithmetic explanation][bearssl] and
[Bernstein–Chou's binary-field work][binary-fields] provide relevant controls.
This can preserve v1 outputs and constant-access behavior. It is unlikely to
beat native CLMUL and should be prioritized only for a real fallback workload.

### 8.3 New algorithms and broader research candidates

**Current priority:** algorithm changes are authorized. Section 9 gives concrete
compositions and supersedes the tentative ordering in this section.

These change the hash definition or security contract. None can silently replace
v1 on one CPU backend. A surviving design needs a complete byte encoding, key
distribution, cross-length/branch proof, bound comparison, frozen vectors, and
the project's formalization before adoption. The old portfolio remains useful
for controls, but the following work broadens it.

#### R1. Revisit carryless plus integer work as a bounded research experiment

The recorded `256 B carryless + 128 B integer` mixture reached 36.4 B/cycle
against 32.0 for carryless alone. Its roughly 13.8% improvement warrants a
small reproducible prototype if bulk gains justify algorithm research; it is
not sufficient to recommend a production v2. First check the exact multiplier,
shuffle, add, load, and frontend costs of the **complete** integer compressor.
The extra vector instructions compete for resources despite separate multiply
capacity. The tested scalar `mulx` mixture lost and has lower priority.

Independently derived direction: use a cheap invertible SIMD encoding across
32-bit word pairs before NH so each nonzero input difference reaches enough
independently keyed products to accumulate 128 bits of collision resistance.
This seeks an integer analogue of chain coding with distance four or higher,
instead of four full duplicated NH streams. [EHC][nandi] and [HalftimeHash][hh]
are the basis, but addition modulo powers of two has noninvertible elements:
distance alone is not a proof of the final difference bound. Search low-cost
codes/matrices with the actual ring and output projection in the model.

If fixed message positions are divided between two independently keyed AXU
compressors and their outputs XORed, a fixed unequal message pair differs in
at least one partition. Conditioning on the other output can bound the result
by that partition's AXU bound; a uniform bound is the worse partition bound.
An AU-only integer digest cannot use this argument until converted to AXU.
Fixed partitioning, full digest projection, length binding, and outer chaining
all need analysis. Never select the mathematical partition ratio by CPU under
one algorithm ID. Measure realistic messages after projection and chunk close.

#### R2. Multi-265: a missed integer-field construction

[Ghosh, Fuchs, Amiri Eliasi, and Daemen (2023)][multi265] give
multiply-transform-multiply hashing with near-MDS matrices; Multi-265 uses
`p = 2^26 - 5` and has a stated `2^-154` Δ-universality bound. Their byte
encoding places 24 data bits in each field word, so packing cost must be counted.
This is distinct from Multimixer-128 and was absent from the earlier portfolio.

Candidate: vectorize across independent blocks with `vpmuludq`, deferred safe
reductions, and a transpose that makes the small circulant transform cheap.
Retain the complete digest and apply independent GF(2^128) projection. The
conditional-collision argument proposes `2^-154 + 2^-128` for that projected
compressor. This is a composition proposal, not the paper's XOR-bound theorem.
Chunking and cross-length handling must fit the target envelope separately.
Do not map arbitrary 26-bit words modulo `p`: that is not injective.

**Priority: medium for an operation-count prototype.** It provides a new route
to strong compression with ordinary multipliers, but prime-field reductions,
packing, mixing, and final projection may lose to the simpler binary kernel.

#### R3. Integer-field BRW and IFMA, with the actual security bound

[Bhattacharyya, Nath, and Sarkar's prime-field study][prime-brw] and the
[2025 vectorized BRW paper][dec-brw] merit explicit coverage. The latter's
decimated BRW construction exposes parallel streams; its AVX2 implementation
reports 0.332 cycles/byte for a 512 KiB input versus 0.425 for its polynomial
baseline. That is a result on the paper's platform, not a Zen 5 forecast or a
comparison against RaijuHash. Its group-difference definition includes modular
addition; "AXU" in that paper must not be read automatically as bitwise XOR.

Explore 26-bit `vpmuludq` limbs and 52-bit IFMA limbs for an appropriately large
prime, comparing Horner folds, BRW leaves, and deferred carries. IFMA's low/high
instructions perform multiply-**accumulate**; dismissing it solely because a
full product uses two instructions misses saved additions and radix choices.
Conversely, count every carry/reduction and prove exact accumulator bounds.
[IFMA arithmetic research][ifma] supplies implementation ideas, not a hash proof.

For the project's tight envelope, a small prime and a 128-bit output do not
automatically suffice. Polynomial degree, key restriction, encoding, projection,
and length all affect the bound. Start with a larger-field compact-key design
and derive its cost, or retain this as a many-key/AVX2 control. A GF(2^256) BRW
variant remains expensive on a carryless-multiply-limited CPU; prime-field and
binary-field costs must be evaluated separately.

#### R4. Automated polynomial design and the 2026 multivariate survey

[Degabriele et al.'s SoK][poly-sok] systematizes prime-field design, limb
representations, and generated implementations. Their [2026 follow-up][multi-poly]
adds multivariate/two-level designs and binary fields. The follow-up's abstract
reports up to 25% over its comparison set and about 0.3 cycles/byte for 128-bit
binary fields, with further vectorization left open. It does not report a win
over this project's ~0.035 cycles/byte warm Zen 5 bulk result, and the CPU and
security parameters differ. The abstract was accessible; the full PDF was not
retrievable in this review, so no detailed theorem or vector schedule is claimed.

Use the framework's methodology to search choices instead of treating one
hand-selected polynomial as representative: field size, two-level leaf size,
degree, key count, precomputed powers, and instruction costs. Rank candidates
by full cost with a proven bound and intended cache regime. The likely useful
result here is a compact-key or setup tradeoff, not an immediate hot-bulk winner.

#### R5. Ahle–Knudsen's injective polynomial schedule

The existing P256 note mentioned [Ahle–Knudsen (September 2026)][ahle]. Their
injective hashing construction uses `N` field multiplications for `2N` input
field values. Keep it on the compact-key list and compare against BRW using
the hashing construction itself. The separate rational-preprocessing results
for evaluating preprocessed polynomials do not make message-dependent
preprocessing free. Confirm characteristic-two applicability, polynomial degree,
and the published version before deriving an AXU wrapper.

Independent cost screen: at GF(2^128), half a field multiplication per 16 B is
still roughly 1.5 base CLMUL products per 16 B with Karatsuba, before reductions.
At GF(2^256), it is roughly 4.5 per 32 B. That loses the base-product contest
against v1's one per 16 B, but may win when avoiding large key tables matters.
The performance target is therefore many keys or small prepared state.

#### R6. Modern AES-based hashes/MACs as explicit alternative contracts

The earlier Pelican/Tachyon-only screen omits substantial published work:
[LeMac/PetitMac (2024)][lemac], its [2025 corrigendum][lemac-fix],
[EliMAC (2023)][elimac], and [SMAC (2025)][smac]. LeMac's published body uses
two AES rounds per 16 B and reports 0.068 cycles/byte on Ice Lake. The
corrigendum distinguishes the original LeMac-0 from corrected LeMac.
[Nagoya et al. (2026)][lemac-analysis] further analyze forgery bounds and the
importance of the padding rounds; its publisher abstract was inspected.

SMAC reports 0.038 cycles/byte for an aggregated mode. That number cannot be
treated as an isolated-message hash latency or proof of the v1 AXU guarantee.
All of these deserve comparisons under their **own** MAC/security assumptions;
active-S-box/differential arguments or block-cipher assumptions do not replace
the algebraic uniform-key theorem of v1. Full initialization, padding,
finalization, and aggregation costs must be included.

Independent throughput screen: two 512-bit AES round instructions/cycle would
process `2 * 64 / 2 = 64` input B/cycle for a perfectly utilized two-round-per-
block body, before XORs and dependencies. The old 32 B/cycle dismissal is not
a universal VAES ceiling. Achieving the wider bound requires enough independent
state or messages; a sequential AES recurrence may be latency limited. This
is a reason to investigate scheduling and optional alternative MACs, **not**
to revive the rejected two-round final permutation in §7.5.

#### R7. Other directions worth recording, with clear stopping rules

| Direction | Why consider it | Why it is not a leading v1 optimization |
|---|---|---|
| Larger chunks in a new version | Amortize endpoint and outer work. | Doubling 8 KiB chunks only changes the inner count from 8.125 to 8.0625 products/128 B: about **0.78%** ideal gain, plus any close-cost saving, while doubling the table. |
| Smaller chunks / compact-key tiers | Lower table/cache/setup cost for many keys. | More endpoints and outer steps; re-derive the length-dependent bound. CPU-dependent chunk sizes would change output. |
| NH-Toeplitz key sharing | Published key reuse can reduce the cost of independent NH streams. | [UMAC's construction][umac] is a specific theorem and encoding; arbitrary shifts of v1's table do not inherit it. |
| Toeplitz/linear convolution hashing | Regular linear algebra; possible CLMUL batching. | Count polynomial products, overlap, extracted output bits, and key size at a true 128-bit bound. Fixed CRC instructions alone cannot implement arbitrary keyed 128-bit hashing. |
| Tabulation | [Simple tabulation][tabulation] avoids multiplies and supplies strong distributional properties. | A byte-position table with 128-bit values costs 4 KiB **per position**; cache traffic and indexed access make it an unattractive general MAC/bulk replacement. Fixed short domains are a separate study. |
| GFNI / VNNI / AMX as an entire new compressor | Different arithmetic may use idle hardware. | Encoding, exact non-saturating sums, rank/distance proof, matrix setup, and projection dominate naive designs. GFNI's 8-bit field is not a 128-bit guarantee. |
| Floating-point/FMA hashing | Exact small-limb products may use other units; historical Poly1305 work is relevant. | Prove every rounding/range bound and account for FP-environment dependence. Prefer exact integer/IFMA controls first. |
| FFT/additive-FFT polynomial products | Large convolutions can amortize multiplication. | Setup, transforms, scratch memory, and general-product work are poorly matched to independent 64×64 products and 8 KiB chunks. Only pursue a concrete many-key or much-larger-block crossover. |
| Fixed-prefix/suffix preparation | Reused packet headers or records permit reusing partial state. | Chunk chain boundaries and final length still matter. Cache a valid prefix state; for arbitrary edits, derive exact affected chunk summaries and outer powers. Not faster for unrelated messages. |
| GPU or accelerator offload | Large resident batches can use more hardware. | Transfers, launch cost, and lack of the same native carryless instruction may dominate. It does not establish faster single-core x86 hashing. |

### 8.4 Order of work and acceptance criteria

**The algorithm-replacement order in section 9.8 now takes priority.** For a
round focused specifically on implementing v1, start with **X1–X3**, then **X6/X7**,
because each has a specific source-level gap and preserves the algorithm.
For setup-heavy users choose **X10/X11**; for existing batch/parallel workloads
choose **X13/X14**. Measure **X8/X15** on the actual Intel/AMD targets before
generalizing the Zen 5 policy. Pursue **R1/R2** as small research prototypes;
**R3–R5** primarily target key footprint. Keep **R6** as an explicitly different
security/API comparison.

For each survivor, a later experiment should record:

1. **Equivalence or new contract:** v1 outputs including seeded derivation,
   independent `R2`, all lengths/tails, fragmented updates, avalanche, and MAC;
   or an explicitly new algorithm with a reviewed bound. No statistical
   collision test substitutes for a universality proof.
2. **Full costs:** construction, first long use, steady state, finalization,
   code size, key bytes, stream bytes, and temporary allocations. For lazy
   expansion, include each newly generated prefix rather than subtracting setup
   as if it were a fixed additive constant.
3. **Representative inputs:** every small boundary, mixed lengths, offsets
   0–63, 1/7/31/127/1024/4096-byte updates, hot/rotating/DRAM buffers, and
   1/16/256/4096 keys. Include dependent latency chains as well as independent
   call throughput: the existing repeated-call timer does not isolate latency.
4. **Real CPUs and reproducibility:** Zen 5, another AMD generation, Intel
   AVX-512, and Intel AVX2/PCLMUL; record exact CPU, features, frequency,
   affinity, compiler, flags, binary revision, and backend. Preserve raw samples
   and prototype source. Use hardware counters for resource diagnoses; static
   scheduling models can be inaccurate for microcoded operations.
5. **Decision:** require an end-to-end win larger than measurement variability
   in the intended workload, with median/tail results and randomized process
   layouts. Do not adopt a tiny warm-loop gain that loses setup or mixed-input
   performance. Revisit a losing idea only with a changed hypothesis or CPU.

These are future validation requirements. This update only changes this document.

### 8.5 Source ledger and reading map

Sources were searched/inspected on 2026-09-28. This ledger distinguishes papers,
standards, original implementation reports, and processor guidance. Descriptions
in this document are paraphrases; the independent proposals are not claims made by the
papers. Linked software is a comparison resource, not code imported into v1.

| Source | What it contributes / scope of reading |
|---|---|
| [Gueron–Kounavis, *Efficient implementation of the Galois Counter Mode using a carry-less multiplier and a fast reduction algorithm* (2010)][gk] | Field multiplication/reduction foundation; publication abstract inspected. |
| [Intel, *Carry-Less Multiplication Instruction and its Usage for Computing the GCM Mode*][clmul] | Author/vendor arithmetic reference for packing and reduction. |
| [Intel, *Enabling High-Performance Galois-Counter-Mode*][gcm] | Full paper accessible; Karatsuba versus schoolbook and aggregation tradeoffs. |
| [Nandi, *On the Minimum Number of Multiplications Necessary for Universal Hash Constructions* (FSE 2014)][nandi] | EHC and model-specific lower bounds; does not certify a processor speed ceiling. |
| [Lemire–Kaser, *Faster 64-bit universal hashing using carry-less multiplications* (2016)][clhash] | CLHash and carryless NH; 64-bit bound must not be confused with this contract. |
| [Kaser–Lemire, *Strongly universal string hashing is fast*][strong] | SIMD arithmetic and setup/key-traffic tradeoffs. |
| [Ivanchykhin–Ignatchenko–Lemire, *Regular and almost universal hashing: an efficient implementation*][pmp] | Integer/superscalar and SIMD control families; paper record inspected. |
| [Apple, *HalftimeHash* (2021)][hh] | Integer EHC, output/bound/implementation tradeoffs. |
| [Ghosh–Amiri Eliasi–Daemen, *Multimixer-128* (2023)][multimixer] and [FSE 2024 slides][multimixer-slides] | Full paper inspected, including equal-length versus variable-length bounds and the matrices used by the proof. |
| [Ghosh et al., *Universal Hashing Based on Field Multiplication and (Near-)MDS Matrices* (2023)][multi265] | Full paper inspected for Multi-265's theorem and byte packing. |
| [RFC 4418, *UMAC*][umac] | NH, independent instances, key sharing, and complete construction details. |
| [Bernstein, *Polynomial evaluation and message authentication* (2007)][brw] | Original BRW background; author-hosted search excerpt accessible, PDF retrieval failed. |
| [Chakraborty–Ghosh–Sarkar, *A Fast Single-Key Two-Level Universal Hash Function*][twolevel] | Binary-field BRW/Horner hierarchy; publication record inspected. |
| [Bhattacharyya–Nath–Sarkar, *Polynomial Hashing over Prime Order Fields*][prime-brw] | Prime-field schedules and costs; distinct from binary-field product accounting. |
| [*Vectorised Hashing Based on Bernstein-Rabin-Winograd Polynomials over Prime Order Fields* (2025)][dec-brw] | Full HTML inspected for decimation, group definition, and AVX2 timings. |
| [Degabriele et al., *SoK: Efficient Design and Implementation of Polynomial Hash Functions over Prime Fields* (S&P 2024; ePrint 2025)][poly-sok] | Systematic design/implementation search. |
| [Degabriele et al., *New Designs of Multivariate-Polynomial Universal Hash Functions* (2026)][multi-poly] | Abstract/metadata only; full PDF unavailable in this review. |
| [Ahle–Knudsen, *Fast Evaluation of Polynomials with Rational Preprocessing* (2026)][ahle] | Abstract and hashing sections inspected; pin version before using a theorem. |
| [Drucker–Gueron, *Fast modular squaring with AVX512IFMA* (2018)][ifma] | Exact radix-52 multiply-accumulate implementation ideas. |
| [RFC 8452, *AES-GCM-SIV*, especially GHASH/POLYVAL relationship][polyval] | Representation conversion and bit-order reference, not a substitute hash. |
| [Bariant et al., *Fast AES-Based Universal Hash Functions and MACs* (2024)][lemac] | Author-hosted full paper; LeMac/PetitMac scheduling and security model. |
| [Bariant et al., *Corrigendum* (2025)][lemac-fix] | Corrected LeMac specification; correction confirmed by the 2026 paper and published corrigendum text. |
| [Nagoya et al., *Analyzing Forgery Security of LeMac* (2026)][lemac-analysis] | Publisher abstract inspected; complete chapter not accessed. |
| [Dobraunig–Mennink–Neves, *EliMAC* (2023)][elimac] | Author publication page and conference slides; alternative MAC comparison. |
| [Wang et al., *A New Stand-Alone MAC Construct Called SMAC* (2025)][smac] | Author-organization paper/abstract, including aggregated-mode timing. |
| [Pătraşcu–Thorup, *The Power of Simple Tabulation Hashing*][tabulation] | Table-based alternative and its distinct workload tradeoff. |
| [Bernstein–Chou, *Faster Binary-Field Multiplication and Faster Binary-Field MACs* (2014)][binary-fields] | Binary-field arithmetic and software-fallback research; author-institution abstract inspected. |
| [BearSSL, *Constant-Time Crypto*][bearssl] | Original implementation explanation of carryless multiplication using integer arithmetic with gaps. |
| [Backtrace, UMASH original repository][umash] | Engineering comparator; its 128-bit fingerprint does not claim a 2^-128 collision bound. |
| [uops.info VPCLMUL][uops-vpclmul] and [PCLMUL][uops-pclmul] | Original instruction measurements; width, CPU, operand, and throughput units matter. |
| [Yee, *Zen5's AVX512 Teardown*][yee] | Original independent hardware experiments supporting the width/issue-rate distinction. |
| [AMD Zen 5 Software Optimization Guide][amd-sog], [Intel optimization manual][intel-opt] | Processor-specific loads, scheduling, instruction delivery, and memory guidance. |
| [Intel crypto/frequency white paper][intel-crypto], [vector-length extensions][vl], [GFNI guide][gfni], [AES-NI paper][aesni] | ISA and deployment guidance for X8, X10, and X16. |
| [Rogaway et al., *OCB: A Block-Cipher Mode of Operation for Efficient Authenticated Encryption*][ocb-field] | Author-hosted field-arithmetic reference, including the degree-64 irreducible polynomial used in section 9.4. |
| [Ahle, *Independent execution-based verification of two HalftimeHash claims* (2026)][hh-audit] | Original investigation inspected; its specific HalftimeHash24 witness was not reproduced here. |
| [Ahle, ChainHash original repository (2026)][chainhash] | Current specification/theorem/design documentation and reported bounds inspected; no implementation imported or benchmarked. |

## 9. Algorithm replacement with the same security or better

**Active objective, following the user's clarification:** maximize x86 speed,
including a new hash definition. Compatibility with v1 digests is not a reason
to reject a candidate. New key formats, message layouts, internal widths,
chunk sizes, and short/bulk branches are available design choices. A frozen
replacement must still give identical results on its different CPU backends.
The user has requested research and documentation at this stage, not code.

The constructions below are independently derived research proposals using
published primitives and the existing proof structure. This is not a claim of
novelty, a completed security audit, or a measured performance result.

### 9.1 What must remain at least as strong

For `Q = 2^128`, require, for every allowed maximum byte length `L < 2^64`,
distinct fixed messages and every target XOR difference:

```text
epsilon_new(L) <= (ceil(L / 8192) + 1) / Q.
```

Compare complete bounds at each length, including partial chunks and branch
boundaries. For the tiny branch, preserve the existing stronger `1/Q` result
as well. Internal output width may exceed 128 bits, but the final 128-bit
output must satisfy the bound. AU (collision-only) compression is sufficient
inside an appropriate AXU construction; it is not sufficient as the final hash.

Preserve the current uniform-key theorem and constant-time treatment of message
and key contents. Seeded key expansion and the MAC have additional guarantees:

- Keep the existing nEHtM MAC as the initial wrapper. A no-worse AXU bound gives
  a no-worse truncated-hash bound `delta = 2 * epsilon_new(L)` in its existing
  theorem, assuming independent hash/cipher keys and the same nonce rules.
- Recalculate the seed-expansion reduction. Increasing the number `s` of
  AES-CTR blocks increases the switching term `s(s-1) / 2^129`; biased
  prime-field sampling adds another issue. Equal uniform-key bounds alone do
  not prove that the complete seeded MAC bound is no worse.
- An alternative MAC must compare concrete forgery bounds at the same message
  lengths, tag/verification counts, nonce-repetition policy, and assumptions.
  A 128-bit tag or a claimed security level does not establish this comparison.
- Match the existing assurance level with a reviewed specification and Lean
  proof before adoption. Performance experiments can precede that work, but
  their provisional security status must be explicit.

### 9.2 Joint outer hashing removes an unnecessary composition penalty

The earlier M32/R1/R2 sketches projected a compressor's digest to 128 bits and
then analyzed an outer hash separately. There is a better composition to try.

Let a keyed chunk compressor have equal-length collision probability at most
`alpha`. Encode its **entire** output injectively as a fixed vector
`Z_i = (Z_i,0, ..., Z_i,r-1)` of `GF(2^128)` elements. With uniform independent
`R, A_1, ..., A_(r-1)` independent of the compressor key, define:

```text
P_0 = 0
P_i = (P_(i-1) + Z_i,0) * R + sum_(j=1..r-1) Z_i,j * A_j
H   = P_q + L * T + S
```

`T,S` are independent field keys, and `q` is the chunk count. The vector
dimension and encoding are fixed for the chosen long-message branch.

**Proof sketch.** For equal-length messages, condition on the compressor key.
If the compressed sequences differ, the output-difference equation is a
nonzero polynomial in `R,A_j`: coordinate zero uses monomials `R^(q-i+1)`;
coordinate `j > 0` uses `A_j * R^(q-i)`. These monomials are distinct and
nonconstant. Total degree is at most `q`, so the root bound is `q/Q`.
If the sequences coincide, the first differing input chunk has collided,
an event of probability at most `alpha`. Thus:

```text
epsilon_equal_length <= alpha + q / Q.
```

Different lengths are separated by independent `L*T`, as in v1. Reusing the
compression table across chunks is allowed by this argument; independent
chunk outputs are not assumed. The multiplication cost is `r` field products
per chunk, before batching/precomputation. Joint analysis saves a **bound
term**, not all the arithmetic of folding a wide digest.

For two compressors assigned disjoint fixed positions, concatenate their full
outputs into `Z_i`. A differing chunk differs in at least one partition, and
collision of the concatenation requires collision in that partition. Therefore
`alpha <= max(alpha_left, alpha_right)` is sufficient. There is no need to
assert that an integer additive-difference theorem is an XOR theorem, or to
XOR independently projected 128-bit outputs. Keep component keys independent.

**Important literature refinement.** Multimixer's [paper][multimixer],
Corollary 4 and section 1.1, gives `2^-128` for equal-length inputs; its
headline `2^-127` bound also covers unequal lengths. Its output is 512 bits.
For equal-length chunks, use `alpha = 1/Q`, retain all four 128-bit words,
and the proposed joint construction targets exactly `(q+1)/Q` at 8 KiB
chunks. [The authors' FSE slides][multimixer-slides] also distinguish the
maximum differential probability from the maximum image probability.
This replaces the conservative M32 projection accounting in section 2.

For Multi-265, `alpha <= 2^-154` gives `q/Q + 2^-154`, slightly better than
v1 at the same chunk size. Its complete 12-residue output fits injectively
in three field elements if canonically packed; padding each residue to a
32-bit word is also injective and still totals three field elements.

**If a future compressor only proves `alpha = a/Q`:** a chunk size `C` and
branch threshold can sometimes absorb the difference without weakening the
final guarantee. Require
`a + ceil(L/C) <= 1 + ceil(L/8192)` at every length in that branch. For
example, `a=2, C=16384` works for `L>8192`, with a stronger short/medium
family below it. This is a security-budget calculation, not a free speedup:
the larger table, final partial chunk, and seeded-key terms still count.

### 9.3 First bulk candidate: carryless plus integer compression

Use the joint outer construction with a fixed public partition of each 8 KiB
chunk between chain-coded carryless compression and Multimixer. Start with
the measured `256 B carryless : 128 B integer` scheduling hypothesis, then
compare alternative fixed layouts before choosing the algorithm definition.

One concrete full-chunk layout has 21 such groups plus a final 128-byte
carryless segment: 5,504 carryless bytes and 2,688 integer bytes. The carryless
subsequence has 43 blocks and one endpoint; its table costs `44*128 = 5,632`
bytes. The integer portion uses 2,688 key bytes. Together that is **8,320
compression-key bytes**, the same as v1's 65-row table, before outer/short keys
and prepared state. Stream the partition directly from the input; do not copy
the subsequences into temporary buffers.

Retain the two carryless field outputs and four integer field outputs as six
coordinates. Under the component theorems, the concatenated compressor has
`alpha <= 1/Q`; section 9.2 therefore targets v1's exact envelope. The
six-product chunk close can itself use wide CLMUL and aggregated reduction.
Combining this with section 9.4 reduces the carryless part to one coordinate,
but that is a second experiment with its own reduction cost.

For partial chunks, specify a fixed assignment of every byte and zero-pad
within the relevant subcompressor. Equal original lengths must induce equal
subsequence lengths and an injective encoding; handle cross-length cases via
`T`. An empty partition is a fixed digest and cannot be the differing one.
Key expansion must cover the actual rows used, in the specified order.

**Why first:** it has local evidence of execution overlap: 36.4 B/cycle versus
32.0 for the old bare loops. That is a measured scheduling opportunity, not
a complete secure implementation or a 13.8% speed limit. Full matrix mixing,
accumulation, tails, and six-coordinate finalization may erase it. Conversely,
better layout and a cheaper integer compressor could improve the overlap.
Do not use the unavailable scratch prototype as a correctness oracle.

### 9.4 Second candidate: two GF(2^64) coordinates with a 128-bit guarantee

The existing chunk proof uses a 256-bit intermediate pair. A field change can
make the pair itself 128 bits while preserving the distance-two argument.

Let `E = GF(2^64)`, for example using `x^64+x^4+x^3+x+1`; this irreducible
polynomial is listed in the [OCB paper][ocb-field]. Apply the same chain
encoding and independent 64-bit key pairs, but define each elementary NH
product in `E`, and combine using columns `(1,j)` for `j<64`, `(0,1)` at
the endpoint of a full chunk. Now each `j` is interpreted in `E`.

**Proof sketch.** For any unequal encoded input pair, the NH output
difference is uniform in `E`: after conditioning on one key word, it is a
nonconstant affine function of the other. Chain distance two supplies two
independently keyed such differences. Two distinct columns are invertible
over `E`, so the chunk's two-coordinate difference has probability at most
`2^-64 * 2^-64 = 1/Q` of hitting any target. Conditioning on other lanes
and positions is the same argument as SPEC Proposition 4.

Pack the two 64-bit coordinates into one 128-bit word `U_i`. This packing is
a bijective XOR-linear map, so the chunk output is already `1/Q`-AXU. Use:

```text
P_i = (P_(i-1) + U_i) * R      # multiplication in GF(2^128)
H   = P_q + L*T + S
```

The same conditioned polynomial argument gives `(q+1)/Q`. For a direct
single-chunk branch, output `U_1 + L*T + S` instead, giving `1/Q` and avoiding
the outer multiplication entirely. Keep the strong tiny-message branch;
different-length branches remain separated by independent length keys.

**Performance opportunity:** one outer field product instead of two; a smaller
chunk result and cheaper combination for medium inputs and parallel workers.
The body still needs 520 base products per full chunk, so this does not
promise a large hot-bulk multiplier speedup. Compare GF(2^64) reduction and
weighted accumulation carefully: linearity permits reducing sums rather than
each product, but the polynomial bits above bit 127 in weighted sums must be
retained or folded correctly. A GF(2^128) remainder cannot simply be re-read
as a GF(2^64) remainder. Include shift/GFNI reductions that avoid occupying
the scarce CLMUL unit. The modulus and the new concrete layout need proofs.

### 9.5 Full integer SIMD compression: layout is part of the search

Run standalone Multimixer through section 9.2 as well as the hybrid. The
previous 21.9 B/cycle result was for a Multimixer-shaped loop; it is evidence
against that schedule, not an optimum for all implementations.

Candidate schedules:

- Assign SIMD lanes to corresponding coordinates of independent blocks. A
  fixed permutation of the chunk's input words is injective, so a new version
  can choose a layout that turns circulant mixing into register additions,
  avoiding repeated in-register transposes. Count strided loads and tails.
- Compare ordinary AoS, groups of several blocks, and a larger interleaved
  tile. Keep all output coordinates until the chunk closes; accumulate in
  multiple vectors if that hides add dependencies without spills.
- Implement the exact two distinct mixing matrices used in the proof, with
  wrapping 32-bit additions and full widening products. Reconcile the
  indexing in Algorithm 2 with Definition 9 before freezing an oracle; the
  proof uses `N_alpha = circ(1,1,1,0)` and `N_beta = circ(0,1,1,1)`.
- Compare direct triple sums with a shared total minus the omitted coordinate.
  Precomputed transformed keys can trade arithmetic for more key loads;
  include that traffic and setup in the comparison.

Multi-265 is the next published control, using its stronger compressor bound
with the same outer framework. It uses more encoding/reduction machinery;
its stronger theorem does not imply higher throughput. Prime-field key
sampling must follow the theorem's distribution. Modulo reduction of uniform
binary words is not an exactly uniform sampler.

### 9.6 Higher-upside research: integer EHC with exact loss accounting

The larger architectural opportunity is a high-rate integer compressor that
uses fewer instructions than Multimixer or the complete HalftimeHash tree.
Investigate fixed 8 KiB EHC leaves and the outer polynomial above, avoiding
an unnecessary integer tree over the entire message.

**Independently derived search criterion.** Suppose `k` changed, independently
keyed NH-32 symbols have additive-difference point probabilities at most
`2^-32`. Let their combination matrix be `A` over `Z/(2^64)`. For any
consistent target, after conditioning on all other positions, its solution
set is a coset of `ker(A)`, hence:

```text
Pr[A * D = target] <= |ker(A)| * 2^(-32*k).
```

For a square integer lift with Smith factors `s_1,...,s_k`,
`|ker(A)| = product_i gcd(s_i, 2^64)`, with `gcd(0,2^64)=2^64`.
Unimodular row/column operations preserve the solution count, reducing the
claim to independent scalar congruences. For full rank with all valuations
below 64, the loss is `v2(det(A))` bits. This can be sharper than applying
the adjugate's worst divisor separately to every coordinate, as in the
conservative [HalftimeHash analysis][hh].

Search for cheap distance-five (or higher) encodings and matrices whose worst
kernel loss is at most `32*k-128`. For `k=5`, up to 32 lost bits would still
meet `alpha <= 1/Q`. All possible differing supports must be covered; a
single failing minor or low-weight encoding difference invalidates the claim.
Distance four with nontrivial kernel loss does not reach the target through
this generic bound. A direct probability proof could be tighter, but must be
supplied rather than assumed.

Include GFNI-assisted byte-field parity, sparse XOR codes, and small integer
coefficients in the offline search cost model. Account for message/key loads,
encoding, shifts, combination, and register pressure, not just products.
VNNI/16-bit NH needs a separately proved base family and more distance; signed
dot products and overflow cannot be treated as unsigned NH without analysis.
This is a speculative design search with potentially greater upside than
rescheduling v1, not a known faster secure algorithm.

**Comparator correction:** a [September 2026 primary investigation][hh-audit]
reports a distance-one encoding defect and a `2^-32` collision witness for
the inspected C++ `advanced::Vj<3>` HalftimeHash24 implementation. It explicitly
does not extend that witness to the standard Style wrappers. This review has
not reproduced the finding or established whether the previously benchmarked
Rust crate shares it. Pin exact implementations and prove their encoding;
do not use a variant name or a published timing as a security certificate.

### 9.7 Other replacements and stronger-security options

| Direction | Position under the clarified objective |
|---|---|
| New short-message family | Allow a separate injective linear/affine family with `1/Q` bound, public-length specializations, independent branch keys, and a compact prepared key. Compare against X1–X5; bulk and tiny paths need not use the same arithmetic. |
| Larger chunks / several chunk tiers | Search 8/16/32 KiB, including the table-cache and seed-expansion costs. Larger chunks can reduce the outer error bound and cost, but will not remove the bulk product bottleneck. Make every threshold part of the specification and prove cross-length cases. |
| Wider internal field | A 192/256-bit outer field followed by independent projection can reduce the length-dependent error substantially. It costs more arithmetic at chunk close and does not by itself accelerate compression. Preserve the complete intermediate encoding and recalculate degree/projection terms. |
| BRW, multivariate polynomials, IFMA | Retain R3–R5 for compact keys and rotating-key workloads. Search the complete design space with the required bound; published throughput at a different bound/platform does not rank above the local hybrid evidence. |
| Modern AES-round MACs | Keep R6 in the comparison set, but changing the algorithm does not authorize weaker assumptions or a worse forgery envelope. A full-round-AES reduction or a reduced-round differential argument must be compared explicitly to the current theorem. |
| ChainHash-128 (2026) | A newly located compact-key comparator. Its authors report a `(p+d)/2^128` collision bound; at 1 MiB the stated numerator is 2,080 versus v1's 129. The published bound therefore does not meet this project's pointwise target. Its layout and deferred-reduction ideas are useful; its 64-bit timings are not 128-bit evidence. See the [original repository][chainhash]. |
| MAC finalization replacement | Rank only after checking nEHtM's actual query bounds. Replacing its two AES calls with a conventional one-pad construction may change nonce-fault and large-query security. The overlap experiment X12 has a simpler security case. |

### 9.8 Active order and realistic performance limits

1. **Specify the generic joint outer lemma and the exact component encodings.**
   It enables fair comparisons at the required bound, without a spurious
   projection penalty. Resolve Multimixer's matrix indexing first.
2. **Prototype the carryless/Multimixer hybrid and standalone integer control.**
   Include full outputs, chunk close, padding, key preparation, and streaming.
   Freeze a CPU-independent partition only after comparing schedules.
3. **Prototype the GF(2^64) two-coordinate compressor.** Compare its direct
   one-chunk branch and its combination with the hybrid. This has a short
   proof path and targets finalization/medium-message cost.
4. **Search integer EHC encodings/matrices and benchmark complete survivors.**
   Compare Multi-265 as a published stronger-bound control. Reject insecure
   parameterizations before treating their timings as candidates.
5. **Retain the best short path and apply X1–X18 where relevant.** Changing
   the bulk algorithm need not sacrifice short-message performance. Complete
   formalization, vectors, and the MAC/seed-bound comparison before adoption.

Do not call 13% or 25% a universal ceiling. On the recorded Zen 5 machine,
the bare carryless issue budget is approximately 32 B/cycle; the integer
multiplier-only budget for a 0.25-product/B compressor is about 64 B/cycle.
Those capacities cannot simply be added: loads, adds, shuffles, decode, and
registers are shared. With one key byte loaded per message byte and two aligned
64-byte loads/cycle, the load-only bound is approximately 64 message B/cycle,
before any other work. Key reuse in registers changes that model. These are
resource bounds, not predictions of a 2x speedup.

The practical objective is the fastest **complete, adequately proved** design
per workload, measured on actual AMD and Intel CPUs. Warm single-core bulk,
fresh/rotating keys, packet latency, batched throughput, and DRAM throughput
can have different winners. The existing 42–53 GB/s DRAM results also mean
that a faster arithmetic core may show no benefit once memory is limiting.
No newly proposed replacement has been implemented or benchmarked here.

## 10. x86 experiments, 2026-09-28

This section reports measurements, unlike sections 8 and 9. The candidates of
§9.8 were implemented as prototypes and timed on AWS `c8a.2xlarge` instances
(AMD EPYC 9R45, Zen 5; Rust 1.98.1, `-C target-cpu=native`). The first host
ran at 4.49 GHz; it was reclaimed, and its replacement ran at 4.24 GHz.
Comparisons below are always within one host. Prototype sources are in
`experiments/`:

| File | Contents |
|---|---|
| [`src/x86.rs`](../../experiments/src/x86.rs) | Scalar oracles and AVX-512 kernels: the hybrid in Rust intrinsics (`h1`, `h2`, `h4`), transposed Multimixer (`m`), the G64 close, the v1-equivalent control (`h0`) |
| [`gen_x86.py`](../../experiments/gen_x86.py) → [`src/x86_asm.rs`](../../experiments/src/x86_asm.rs) | Hand-scheduled inline-asm hybrid bodies (`hyb4`–`hyb32`, `_seq` controls) |
| [`src/bin/cand.rs`](../../experiments/src/bin/cand.rs) | Checks every kernel against its oracle, then times it against v1 `Key::hash` |
| [`src/bin/xmix.rs`](../../experiments/src/bin/xmix.rs), [`src/bin/xport.rs`](../../experiments/src/bin/xport.rs) | Bare mixed loops; instruction-throughput and port probes |
| [`src/bin/g64.rs`](../../experiments/src/bin/g64.rs), [`src/bin/lat.rs`](../../experiments/src/bin/lat.rs), [`src/bin/off.rs`](../../experiments/src/bin/off.rs), [`src/bin/batch.rs`](../../experiments/src/bin/batch.rs), [`src/bin/nhs.rs`](../../experiments/src/bin/nhs.rs) | G64 against v1's close; latency and MAC timing; input alignment; batched short hashes and MACs; the NH short/medium family |

The prototype definitions are complete (full chunks, chunk close, joint outer
polynomial of §9.2, `L T + S`) but are not frozen specifications: they hash
whole chunks only and have no security review beyond §9.

### 10.1 What Zen 5 offers (port probe)

Independent instructions in asm loops, cycles from a dependent-add clock:

| Instruction mix | Cycles | Reading |
|---|---:|---|
| 4 `vpclmulqdq` zmm | 8.0 | one per 2 cycles |
| 8 `vpmuludq` / 8 `vpsrlq` / 8 `vpaddd` / 8 `vpternlogq` | 4.0 / 4.0 / 2.0 / 3.1 | 2, 2, 4 and ~2.6 per cycle |
| 8 aligned 64-byte loads (unaligned) | 4.0 (8.0) | 2 per cycle (1) |
| 4 clmul + 16 `vpmuludq`, + 16 `vpsrlq`, + 16 loads | 8.0 each | no conflict |
| 4 clmul + 16 / 24 `vpaddd` | 8.0 / 9.0 | clmul takes ~3 of the 4 vector ALU slots |
| 4 clmul + 24 `vpternlogq` | 9.4 | |
| 4 clmul + 8 mul + 8 shift + 8 add | 9.0 | |

Resource model: four vector-ALU slots per cycle, a carryless multiply issuing
every other cycle and using about three of them. A v1 block (two multiplies,
three ternary XORs) leaves about seven slots unused per four cycles.
Transposed Multimixer needs about 72 vector ops per 512 bytes. The model
predicted the measured Multimixer-only rate (29.3 against 29.2 B/cycle) and
the hand-scheduled 16:1 hybrid body (64 against 66 cycles). Hardware counters
agree: v1's bulk kernel runs 3.7 ops/cycle limited by the multiplier, while the
hybrid and Multimixer kernels saturate at 4.2–4.3.

### 10.2 Carryless + Multimixer hybrid (§9.3) and integer-only (§9.5)

Layout: v1's chain-coded compressor on 64 blocks per chunk (its digest is
exactly v1's `(h0, h1)` on those bytes), plus Multimixer-128 tiles of 512
bytes in a transposed layout (word `16c + b` is coordinate `c` of Multimixer
block `b`), so the circulant sums are vertical additions. Chunk output: six
field elements into the §9.2 joint polynomial. The Multimixer matrices follow
the paper's Definition 9 (`N_alpha = circ(1,1,1,0)`, `N_beta = circ(0,1,1,1)`).
All kernels matched their oracles.

Bare loops, L1-resident (B/cycle): carryless alone 31–32; Multimixer alone
29.2; mixes of 2–16 blocks per tile 32–37, best **36.8** (one 512-byte tile per
four blocks). The hand-scheduled 16:1 body on a hot buffer: **38.8**; the same
body streaming real data back to back: 36.7. Those are 15–21% above the bare
carryless loop.

Complete prototypes against v1 in the same run (GB/s at 256 KiB on the first
host, v1 = 131–132):

| Prototype | GB/s | vs v1 |
|---|---:|---:|
| `h0`: v1's compressor, reimplemented (control) | 130–133 | 1.00 |
| `m`: Multimixer only, 8 KiB chunks | 117 | 0.89 |
| `h2`: 8 blocks + 2 tiles per group, 16 KiB chunks | 139 | **1.055** |
| `h4`: 4 blocks + 1 tile, 16 KiB chunks | 133 | 1.01 |
| `hx8` / `hx16` / `hx32`: asm, 8 / 16 / 32 blocks per tile | 135 / 126 / 125 | 1.02 / 0.95 / 0.95 |

Beyond L3 every hybrid is at or below v1 (4 MiB: 91–106 against 106–113).

Where the loop-body advantage went (16:1, second host, v1 = 29.3 B/cycle):
body back to back 36.7 B/cycle; with the Rust wrapper but no close 33; with
the six-coordinate close 28. The close is about 170 instructions per chunk,
and the body already saturates the vector pipes, so every close op costs
throughput. A Karatsuba outer product with shift reduction (8 instead of 12
multiplies) changed nothing; neither did removing the `P` dependency between
chunks, so the cost is instruction count, not latency. Two prototype
artifacts were found and removed on the way: LLVM hoisted every key load out
of the chunk loop and spilled the table to the stack, and 64-byte key loads
from a `Vec<u8>` split cache lines.

**The whole hybrid in asm.** [`gen_x86.py`](../../experiments/gen_x86.py)
also emits `hybfull8/16/32`, which hash every chunk of a message in one asm
block: loop body, the six-coordinate close (endpoint, planes, `h1`,
Multimixer lane sums, joint outer product, shift reduction) and `P` carried
between chunks. Only group totals, keys and `P` go through memory. They match
the oracle. On a hot 40 KiB buffer, back to back, the 16:1 kernel takes 325–328
cycles per 10 KiB chunk against 279 for the body alone. The close costs about 47
cycles, and the hybrid runs at 31.4 B/cycle: **7.5% above v1** (29.2). With data
streamed from L2 (60–256 KiB), the same kernel runs at 1.00–1.02× v1, and
1.02–1.04× with a 2 KiB software prefetch. From L3 (4 MiB) it runs at 0.70–0.94×.
The body is op-bound, so it has no slack for extra load latency, while v1 is
multiply-bound and does. (The 1.13–1.16× readings at 10–12 KiB are a size
artifact: v1 takes its partial-chunk path there.)

**Assessment.** On Zen 5, the carryless + Multimixer hybrid is worth about 7%
on hot data and 0–4% on data streaming from L2, and loses beyond L3. Even that would require a new definition, a larger key (65 rows plus 2–8 KiB
of tiles), the joint-outer and Multimixer proofs in Lean, and reoptimization
for NEON. It is not worth adopting. The earlier bare-kernel +13% does not
survive the chunk close and memory streaming.

### 10.3 GF(2^64) coordinates (§9.4), one-chunk branch

Same position code for both closes, fresh chunks of 1–8 whole blocks, both
checked against oracles. The G64 output is `(h0, h1)` reduced in
`GF(2)[x]/(x^64 + x^4 + x^3 + x + 1)`, plus `(L T0, L T1)` and `S`, with no
multiplication by `R` or `R2`.

| Close | Throughput vs v1's close | Latency (dependent chain) |
|---|---:|---:|
| First version (three lane folds, two-step length reduction) | 0.85–0.97× | 20–29% lower |
| Leaner version (merged folds, one-step length reduction) | 0.97–1.04× | 10–15% lower |

Only latency improves. The body is unchanged, so bulk gains nothing. Not worth
a new algorithm by itself; it could be part of a future v2 made for other
reasons (its one-chunk bound is `1/Q`).

### 10.3a A new short/medium family: NH over GF(2^128) (§9.7)

Derived and prototyped in [`nhs.rs`](../../experiments/src/bin/nhs.rs). One
class per size (≤31, 32–63, 64–127, 128–255, 256–511 bytes), each with its own
independent keys `K_i` and offset `S`. A message is zero-padded to the class
size `c` with its length in the last byte, read as field elements
`X_0..X_{2n-1}`, and hashed as

```text
H = sum_{i<n} (X_i + K_i)(X_{i+n} + K_{i+n}) + S.
```

In characteristic 2 the `K_i K_{i+n}` terms cancel in a difference, leaving
`sum_i K_{i+n} dX_i + K_i dX_{i+n}` plus key-free terms. This is affine in the
key, with a nonzero coefficient whenever the encodings differ, so each class
is **exactly `1/Q`-AXU**. v1 gives `1/Q` below 32 bytes and `2/Q` from 32
bytes. Classes and the long branch are separated by independent `S`. The
pairing `X_i ↔ X_{i+n}` keeps whole 512-bit vectors aligned: a class costs
`c/128` groups of four vector multiplies, one shift reduction and one lane
fold, with no chain positions or outer polynomial. The SIMD code matches its
oracle for every length 0–511.

Against v1 `Key::hash` (same host, ns):

| Bytes | v1 | NH | v1/NH | Store-then-hash latency, v1 → NH |
|---:|---:|---:|---:|---:|
| 1–31 | 4.5–5.8 | 3.1 | 1.43–1.85 | 10.7–11.6 → 12.5 |
| 32–63 | 4.5 | 3.9 | 1.13 | 11.4–11.6 → 14.4–14.6 |
| 64 | 4.5 | 4.3 | 1.04 | 11.6 → 14.1 |
| 65–127 | 6.1 | 4.3 | 1.41 | 19.2 → 13.8 |
| 128–255 | 6.1–7.4 | 5.0–5.2 | 1.18–1.49 | 19.2–19.9 → 14.5–14.7 |
| 256–511 | 7.4–8.9 | 7.4–7.8 | 0.95–1.20 | 19.9–20.2 → 15.8–16.1 |

Below 64 bytes the table uses the lean kernel (`nh_lean`: 256-bit loads,
128/256-bit multiplies and a two-multiply reduction, with no cross-lane
512-bit shuffles). The 512-bit version took 4.1–4.2 ns and about 15.7 ns
store-then-hash latency there. The remaining 1–3 ns of store-then-hash latency
comes from the serial reduction; v1's 32–64-byte path avoids it through its
structure. NH spends two base products per 16 bytes against v1's one, so v1
overtakes it from about 512 bytes. Karatsuba products (three multiplies per
pair, middle correction applied once to the sums) were 6–12% slower for the
64–511-byte classes. The extra folds and XORs cost about as many vector slots
as the saved multiply (§10.1). Adopting it means a v2 definition:
short/medium branches for messages below 256 bytes, about 0.5 KB of extra key,
new vectors and a short Lean proof (affine-in-key uniformity plus branch
separation). It keeps v1's bulk. This is the only replacement found that gives
large gains on its target workload with an equal or better bound.

### 10.4 Integer EHC (§9.6): analysis only

Not prototyped. With modular key addition, one NH-32 product has point
probability `2^-32` for a difference in one word but `2^-31` when both words
differ (the carry case; the Multimixer paper's bilateral bound). Four
differing symbols therefore give only `2^-124`, and distance 5 is needed.
Binary distance-5 codes (e.g. shortened BCH [31,21,5]) or Reed–Solomon parity
over GF(2^8) cost more encoding operations and accumulator registers than
Multimixer's 72 vector ops per 512 bytes. Given 10.1, the integer part's ops
per byte is what matters, so these do not improve the hybrid. Multi-265 (R2)
needs more packing and reduction work per byte than Multimixer and was not
built.

### 10.5 v1 implementation items

| Item | Result |
|---|---|
| X17: `Key::hash`/`Hasher::finalize` of 65 bytes–1 KiB through a dedicated `avx512::group` | **Adopted.** One-shot −9 to −16%, streamed −3 to −12%, latency −13 to −16%, MAC −7 to −12%. Routing through the general `avx512::tail` instead gained only 2–6%. Results |
| X1: packed short path (1–31 bytes) | Throughput −20 to −29% at 16–31 bytes, but +4 ns latency when the message was just written. The 256-bit variant cost 2 ns of latency and gained less. Not adopted (trade-off, not a win). |
| X1 applied to the avalanche multiply | Slower (latency-bound single product). Not adopted. |
| X2: Karatsuba in `mix` | 0.98–1.03×, within noise. Not adopted. |
| X6: streaming 2–4 KiB | Analyzed. `update` absorbs all groups and stores folded sums, and `finalize` reloads them for the endpoint and close; below 1 KiB the cost is the copy. About 7 ns at 4 KiB, spread over several steps, with no single defect. |
| X9: chunk-close bubbles | Bulk runs within about 4% of its multiplier bound (280 against 268 cycles per chunk on the second host), so at most a few percent is available. |
| X10: key setup | Profiled `from_seed`: AES-CTR about 36%, `finish_init` 16%, and a `memcpy` of the whole 9.3 KB `Key` out of the constructor's local, up to about 55%. Inlining the constructors cut `one setup` by 23% but made `compare --setup` 6% slower for keys and 39% slower for MACs: whether the move is elided depends on the caller. Not adopted. A robust fix needs an in-place constructor (e.g. into `&mut MaybeUninit<Key>` or a `Box<Key>`), which is an API decision. |
| X3: reduction per path | Shift reduction instead of the two-multiply reduction in the small and final-chunk closes: 3–10% slower and 1–2 ns more latency (65 bytes–4 KiB). The current choice (multiplies there, shifts in bulk) is confirmed. |
| X12: MAC masking | Both AES blocks in one 256-bit VAES chain (11 instructions instead of 21): MAC latency +1 to +3 ns, throughput +0.4 to +1 ns. The nonce-only block can no longer start before the hash is done; the existing two AES-NI chains overlap better. Not adopted. |
| X13: batches of short messages | Prototype [`batch.rs`](../../experiments/src/bin/batch.rs): four messages of 0–31 bytes per call, one per 128-bit lane, eight vector multiplies per four messages. **1.5–1.6 ns per message against 4.6–6.4 for `Key::hash` (3.0–4.3×)**; a four-tag MAC with both AES blocks of every tag in two 512-bit VAES chains, **2.6–2.7 ns per tag against 8.5–9.0 (3.2–3.5×)**. Outputs equal `Key::hash`/`Mac::tag` for every length. Karatsuba (six multiplies) gains nothing: assembling the lanes is the limit. Needs a new API (`hash_many`/`tag_many`), so it is left for a decision. In this harness, `Mac::tag` of 24–31 bytes also measured 22 ns, but only about 10 ns in an isolated loop: a layout-dependent stall like the one in the optimized results, not reproducible on its own. |
| X17 for 1–8 KiB | Moving `tail`'s multi-group branch into its own function made 1025 bytes–64 KiB 3–12% slower: the vector state then crosses a call through memory. Counters at 1–4 KiB show about 4.5–4.8 ops per cycle, near the vector-pipe limit: at 2 KiB, 40 multiplies (160 microcode ops) plus about 250 other vector ops fit in about 100 cycles against 108 measured. There is no large overhead left in this range. |
| X7/X17: one-group entry for SSE and AVX2 | The same dedicated entry for the generic backends (forced on Zen 5) was 3–19% slower from 128 bytes to 1 KiB and faster only at 65 bytes. Their generic `finish` already compiles well; the gain was specific to the AVX-512 kernels. Not adopted. |
| One-group entry for NEON (Apple M1) | The same dedicated entry for NEON: one-shot 1–2.5% faster at 65 bytes–1 KiB, streamed unchanged. The NEON path was already lean (per-length `small::<NB>` kernels kept out of line). The gain is at the level of measurement variability, so it was not adopted. |
| X14: parallel hashing | **Adopted (small).** The ordered combine and `R^m` now use the backend's hardware field multiply (the avalanche's helper) instead of `portable::gf_mul`. Outputs unchanged; 16 MiB with 2–4 threads gains about 2–4%, and other sizes are within noise. The combine was never large. The real cost is creating scoped threads on every call: with two threads, 1 MiB runs at 52 GB/s against 112 single-threaded, and 4 MiB with eight threads at 71 against 103. A persistent worker pool (an API or dependency decision) is the fix. Prototype [`pool.rs`](../../experiments/src/bin/pool.rs) on the v1 compressor, 128 KiB tasks, results equal to the serial hash, GB/s: at 256 KiB / 1 MiB / 4 MiB, serial 125 / 99 / 101, spawning per call with two threads 16 / 55 / 101, **a two-thread pool 150 / 152 / 152**, and an eight-thread pool 85 / 116 / **215**. From 16 MiB up, both are memory-bound, and this simple spinning pool trails spawning (16 MiB, eight threads: 159 against 192). A production pool would need parking idle workers and a size-dependent thread count. |
| X18: portable carryless multiply | The portable backend reaches about 4.4 GB/s in bulk on Zen 5 (1 byte per cycle). An alternative 64×64 carryless product, [`pclmul.rs`](../../experiments/src/bin/pclmul.rs), splits each operand into five bit classes (every fifth bit): 25 multiplies of 64 by 64 bits into 128, with at most 13 terms per bit, which fits the 4-bit gaps. It agrees with a bitwise product on 200,000 random pairs and the extremes. On the Apple M1 it is only 1.04× faster than the current Karatsuba over 32-bit classes (48 multiplies), because a 128-bit product takes two instructions there. The Zen 5 measurement was lost when the spot instance was reclaimed. Not adopted. |
| X15: input alignment | Offsets 1, 8, 16, 32 and 63 bytes from a cache line change throughput by −6% to +2% from 256 bytes to 256 KiB. Split loads halve load throughput, but v1 needs only about one load per cycle. No action needed on Zen 5. |

### 10.6 Where this leaves the objective

On Zen 5, v1's bulk kernel is within 4–5% of what its algorithm allows, and
the best replacement found adds 7.5% on hot data and 0–4% from L2, and
would need a new algorithm, key, and proofs. Short and medium messages had implementation
headroom; the adopted change takes part of it. Two larger gains remain for
short messages. A batch API gives 3–4× per message (X13, same outputs). A
new short/medium branch, NH over GF(2^128) (§10.3a), gives 1.1–1.85× below
256 bytes with a `1/Q` bound, but needs a v2 definition and proof. Before choosing a v2, the
next measurements should be Intel AVX-512 and AVX2-only CPUs. Their
carryless-multiply throughput differs (§8.2 X8), and it decides both v1's
ceiling and the hybrid's value.

### 10.7 Second round, 2026-09-29

The same instance type, a new host (4.46–4.50 GHz). Every item, with its
numbers and the reason for keeping or rejecting it, is in
[progress.md](../../progress.md). The adopted changes and their before/after
measurements are in the results.
All of them keep v1's outputs.

| Item | Result |
|---|---|
| X10: the constructors' 9 KB move, and the eager AES batch | **Adopted.** Filling a `&mut MaybeUninit<Key>` out of line lets LLVM pass the caller's return slot (102 → 69 ns), and the eager rows and field blocks come from one 20-vector VAES batch (→ 60 ns); `Mac::from_seed` 116 → 75 ns; no API change. Two eager rows only (49 ns) was a trade-off against fresh keys hashing 65 B–1 KiB and was not adopted |
| X4/X1: short path, new form | **Adopted.** With `x^64 A`, `x^64 B` prepared, `x A` needs one fold multiply instead of a two-step reduction. The AVX-512 path is branchless (one 256-bit masked load of `[X0 \| X1]`), and so is AVX2 (dword-masked load plus clamped byte loads). AVX-512 1–31 B: 4.5–6.0 → 2.7 ns; random lengths 11.6 → 2.9 ns, a mispredicted `partial16` branch that fixed-length benchmarks never showed. Unlike the packed short path of §10.5, latency improves too |
| Output products | **Adopted.** `Key::pk` holds `x^64 (R, T, R2, 0)`: the packed `h0 R + h1 R2 + L T` and the generic closes reduce with one fold. AVX-512 65 B–1 KiB 3–11% faster, bulk +1–3% (133 GB/s at 256 KiB); the same for the avalanche and for `L T` at 32–64 B (4.5 → 3.6 ns) |
| X7: SSE/AVX2 medium | **Adopted.** A padded last block of 16-byte pieces made AVX2's 32-byte loads miss store forwarding (65 B: 24 → 12.6 ns); the per-plane `x^b R2` products of the final-chunk closes became `shifted_sum` plus one product (AVX2 1 KiB −23%); AVX2 32–64 B uses 256-bit multiplies (8.7 → 4.9 ns one-shot) |
| X18 on Zen 5 | Rejected: five bit classes 0.52× the current Karatsuba |
| X16/X3: shift or GFNI fold in the new short path | Rejected: throughput −6% but latency and MAC +3–4% (shifts compete with AES for the vector pipes); GFNI is worse by operation count |
| X5, R3 (IFMA) | Rejected by operation count. X5's linear terms alone equal the current body; for 32-bit products IFMA saves no vector op. IFMA itself issues 2 per cycle beside the carryless multiply |
| R6 variant: AES-PRF + carryless hybrid | Rejected. The port probe showed `vaesenc` zmm at 2 per cycle, not competing with `vpclmulqdq` (4 clmul + 16 vaesenc = 8.0 cycles). In a body next to v1's chain, though, AES competes with the chain's XORs: +8% at best in a bare loop ([`aesmix.rs`](../../experiments/src/bin/aesmix.rs)), and the bound would add an AES PRP term for every key (§9.1) |
| 32-byte-store copy into the AVX2 hasher buffer | Rejected. It fixes AVX2 streamed 65 B (23 → 17 ns) but slows other streamed sizes or, placed in the inlined `update`, AVX-512's streamed short messages. Open for measurement on a real AVX2 CPU |

After this round, the NH short/medium branch of §10.3a is 1.2× faster below
32 bytes (was 1.43–1.85×), equal at 32–64 bytes, and still 1.2–1.5× faster at
65–255 bytes. The case for a v2 short branch is now mostly the 65–255-byte
range. Remeasured on this host, the batch prototype of X13 (`batch.rs`) is
1.9× faster than `Key::hash` of 8–31 bytes (1.49 against 2.85 ns per message;
it was 3–4×) and 2.7× faster than `Mac::tag` (2.49 against 6.79 ns per tag).
The SSE backend is what CPUs with AVX2 but no VPCLMULQDQ select (Haswell to
Coffee Lake, Zen 1/2). It got the same branchless loads with 128-bit products:
random 1–31 B 11.5 → 5.4 ns (forced on Zen 5). AVX2 32–64 B uses 256-bit
multiplies (8.7 → 4.9 ns). For many active keys (X11), `Key`'s fields were
reordered so that a message of up to 64 bytes reads adjacent cache lines:
with 4096 keys, a 48-byte hash takes 7.2 → 5.6 ns (probe `keys.rs`).
`hash_parallel` (X14) now works on the calling thread too and uses at most
one thread per 2 MiB, with 256 KiB tasks: 16 MiB with eight threads goes from 215 to 275 GB/s,
and below 4 MiB it no longer falls under serial speed (it was 21–91 GB/s
at 1–2 MiB).


[gk]: https://cris.haifa.ac.il/en/publications/efficient-implementation-of-the-galois-counter-mode-using-a-carry/
[clmul]: https://cdrdv2-public.intel.com/724272/carry-less-multiplication-instruction.pdf
[gcm]: https://www.intel.com/content/dam/www/public/us/en/documents/software-support/enabling-high-performance-gcm.pdf
[nandi]: https://eprint.iacr.org/2013/574
[clhash]: https://arxiv.org/abs/1503.03465
[strong]: https://arxiv.org/abs/1202.4961
[pmp]: https://arxiv.org/abs/1609.09840
[hh]: https://arxiv.org/abs/2104.08865
[multimixer]: https://eprint.iacr.org/2023/1357
[multi265]: https://eprint.iacr.org/2023/696
[umac]: https://www.rfc-editor.org/rfc/rfc4418
[brw]: https://cr.yp.to/antiforgery/pema-20071022.pdf
[twolevel]: https://eprint.iacr.org/2016/1103
[prime-brw]: https://eprint.iacr.org/2023/634
[dec-brw]: https://arxiv.org/html/2507.06490v1
[poly-sok]: https://eprint.iacr.org/2025/464
[multi-poly]: https://eprint.iacr.org/2026/1602
[ahle]: https://arxiv.org/abs/2609.06022
[ifma]: https://eprint.iacr.org/2018/335
[polyval]: https://www.rfc-editor.org/rfc/rfc8452
[lemac]: https://who.rocq.inria.fr/Gaetan.Leurent/files/LeMac_ToSC24.pdf
[lemac-fix]: https://doi.org/10.46586/tosc.v2025.i1.623-628
[lemac-analysis]: https://link.springer.com/chapter/10.1007/978-981-92-3012-9_4
[elimac]: https://dobraunig.com/publication/elimac/
[smac]: https://www.ericsson.com/en/reports-and-papers/research-papers/a-new-stand-alone-mac-construct-called-smac
[tabulation]: https://arxiv.org/abs/1011.5200
[binary-fields]: https://research.tue.nl/en/publications/faster-binary-field-multiplication-and-faster-binary-field-macs/
[bearssl]: https://bearssl.org/constanttime.html
[umash]: https://github.com/backtrace-labs/umash
[uops-vpclmul]: https://uops.info/html-instr/VPCLMULQDQ_YMM_YMM_YMM_I8.html
[uops-pclmul]: https://uops.info/html-instr/PCLMULQDQ_XMM_XMM_I8.html
[yee]: https://www.numberworld.org/blogs/2024_8_7_zen5_avx512_teardown/
[amd-sog]: https://docs.amd.com/v/u/en-US/58455_1.00
[intel-opt]: https://cdrdv2-public.intel.com/821612/248966-Optimization-Reference-Manual-V1-050.pdf
[intel-crypto]: https://www.intel.com/content/dam/www/central-libraries/us/en/documents/cryptography-processing-with-3rd-gen-intel-xeon-scalable-processors-19-may-2021.pdf
[vl]: https://www.intel.com/content/www/us/en/developer/articles/technical/the-intel-advanced-vector-extensions-512-feature-on-intel-xeon-scalable.html
[gfni]: https://builders.intel.com/docs/networkbuilders/galois-field-new-instructions-gfni-technology-guide-1-1639042826.pdf
[aesni]: https://www.intel.com/content/dam/doc/white-paper/advanced-encryption-standard-new-instructions-set-paper.pdf
[multimixer-slides]: https://iacr.org/submit/files/slides/2024/fse/fse2024/2023_3_34/slides.pdf
[ocb-field]: https://www.cs.ucdavis.edu/~rogaway/ocb/ocb-full.pdf
[hh-audit]: https://thomasahle.com/blog/adversarial-examples-for-hashes/verify/halftime/README.html
[chainhash]: https://github.com/thomasahle/chainhash

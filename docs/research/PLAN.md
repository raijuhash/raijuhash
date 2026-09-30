# Plan for an independent, faster 128-bit hash

> Research notes kept for their reasoning. The raw result files they cite (`results/`) were removed before publication; current measurements are in [BENCHMARKS.md](../BENCHMARKS.md).

Status: **implementation and research plan; no new hash is implemented or
validated yet**. Prepared 2026-09-27. Supporting documents:
[candidate portfolio](CANDIDATES.md), [SIMD work plan](SIMD.md),
[benchmark plan](../BENCHMARKS.md), and [licensing/provenance](PROVENANCE.md).

## 1. Objective and working assumptions

Build a readable Rust implementation with low measured application costs.
Compare several mathematical families before choosing the core,
including alternatives for bulk throughput, tiny-message latency, fragmented
streams, and compact keys. Treat throughput, setup time, key footprint, and
security as separate constraints.

There is no defensible promise of the fastest hash on every CPU and input size.
The current bulk kernel is already strong. Success means a reproducible win for
a stated workload, with an explicit guarantee and an honest regression table.

Confirmed requirements: **preserve keyed universal hashing and optional MAC**;
prioritize **NEON, AVX-512, modern CPUs, and every useful SIMD optimization**.
Exact message/update sizes and key-reuse frequencies remain unspecified. Use
these requirements and remaining defaults for planning:

- NEON/PMULL and AVX-512/VPCLMUL are primary accelerated targets. Cover modern
  x86-64 and AArch64 broadly, then evaluate additional instruction sets as laid
  out in [SIMD.md](SIMD.md). Portable Rust remains correct on other supported CPUs.
- A stable, architecture-independent 128-bit output and byte-stream API.
- Preserve keyed AXU hashing, including its secret-key and independent-message
  assumptions, and implement MAC support behind an optional feature. An empirical
  non-cryptographic mixer does not meet the confirmed requirement.
- Aim for a raw AXU bound no worse than
  `(ceil(L / 8192) + 1) / 2^128` for messages of at most `L` bytes, under uniform
  independent parameters. Any seed-expansion assumption is stated separately.
- Optimize prepared-key reuse first. Measure fresh-key and many-key workloads
  and explicitly report any loss there.
- Store an algorithm/version identifier wherever digests persist; algorithm
  changes require rehashing or a versioned reader.
- Proposed license for wholly new code: 0BSD, subject to the provenance policy.

The implementation sequence can start under these defaults. Before publishing
performance claims, replace the provisional workload priorities with real
input-size, update-size, CPU, and key-reuse distributions.

## 2. Fixed security contract and comparison algorithms

| Role | Implementation direction | Acceptance constraint |
|---|---|---|
| Required keyed universal core | Screen the six families in [CANDIDATES.md](CANDIDATES.md); section 3 is the worked U8 example. | Check each encoding/bound, benchmark prototypes, then complete the selected proof. |
| Required optional MAC feature | Implement and review a published MAC composition using the new universal core. | Preserve the documented security target and bounded nonce-reuse contract. |
| Non-cryptographic speed references | Include XXH3-128, with rapidhash in a separately labeled 64-bit category. | These are comparison points and do not satisfy the required security contract. |
| Established cryptographic reference | Include BLAKE3 when contextualizing public-integrity or keyed-MAC costs. | Do not equate its properties or workload with raw universal hashing. |

XXH3 supplies a stable 128-bit non-cryptographic variant, while the official
BLAKE3 project supplies a cryptographic hash and keyed mode.
[xxHash documentation](https://github.com/Cyan4973/xxHash),
[BLAKE3 documentation](https://github.com/BLAKE3-team/BLAKE3).

Neither concatenating two related 64-bit outputs nor adding an avalanche function
establishes the required 128-bit bound. A non-cryptographic adoption option is
outside the confirmed scope; it cannot be used to declare this task successful.

### Candidate selection before implementation

The plan now considers six families. U8 is a worked example, not the selected
winner. The [candidate catalogue](CANDIDATES.md) specifies constructions, proof
paths, expected costs, first experiments, and rejection criteria:

| Family | Intended advantage | First comparison |
|---|---|---|
| Wider EHC, U8/U16/U32 | Reduce parity overhead per input byte. | U16 versus U8 on NEON and AVX-512, including tail/state costs. |
| Multimixer-based M32 | Use ordinary 32-bit SIMD multiplication in the body. | Complete compression plus projection against carryless EHC. |
| Compact-key P256 | Replace a large per-position key table with polynomial parameters. | Fresh-key and many-key cost versus bulk arithmetic cost. |
| Field-linear L128 | Simple 16-byte streaming units and regular field operations. | Short/medium latency, fragmented updates, and state size. |
| Four integer NH streams, I4 | Native integer arithmetic for the portable/SIMD body. | Platforms without fast polynomial instructions; key bandwidth. |
| Dual carryless NH, D2 | Eliminate parity and combination-matrix work. | Whether regular scheduling offsets the extra products/key loads. |

Investigate U16, M32, and P256 first because they explore different tradeoffs.
Use L128, I4, and D2 as bounded controls or workload-specific alternatives.
Prove the final selected composition and freeze its output only after this
comparison. A single algorithm identifier must still produce identical results
on every CPU; different families cannot be silently selected by ISA.

## 3. Worked example: U8

Working designation: **U8 candidate**, one member of the wider-EHC family.
This is a proposed composition with a proof outline, not an established or
audited algorithm. The parameters in this section and its streaming layout are
specific to U8. Other families have their own layouts and proof obligations.

The public starting points are independently keyed encode-hash-combine (EHC)
and carryless NH. EHC uses an encoding with a minimum distance, independent keys
for its component hashes, and an invertible-column combination matrix. It can
amplify a component differential bound. These principles are documented in
[Nandi's EHC paper](https://eprint.iacr.org/2013/574) and
[HalftimeHash, section 3.3](https://arxiv.org/html/2104.08865v2#S3.SS3).
Carryless multiplication for universal hashing is analyzed by
[Lemire and Kaser](https://arxiv.org/abs/1503.03465).

The choices below are this plan's proposed instantiation. They require an
independent proof and implementation; they are not claims taken from those papers.

### 3.1 Specify the field and encoding first

Use `F = GF(2^128)` with modulus `x^128 + x^7 + x^2 + x + 1`. Interpret bit `i`
of a little-endian `u128` as the coefficient of `x^i`. Write explicit conversions
for every load, store, and vector lane. This polynomial is documented in
[NIST SP 800-38D, section 6.3](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication800-38d.pdf).
The proposed bit encoding differs from GHASH's external bit convention; published
GHASH vectors need an explicit conversion before being used as field tests.

Use separate notation for a full 64 × 64 carryless product and multiplication
in `F`. A carryless product fits in 128 bits; multiplication of two field
elements also needs reduction. An unreduced product must never accidentally
stand in for a field multiplication.

### 3.2 Tiny messages: 0–31 bytes

Injectively encode the message into 32 bytes by appending a byte `0x01` and then
zeroes. Load the result as two field elements `X0, X1`. Use independently uniform
field parameters `A, B, S_short`:

```text
H_short(message) = A * X0 XOR B * X1 XOR S_short
```

Here and below `*` denotes multiplication in `F`. Distinct short messages have
distinct encodings, so at least one nonzero difference coefficient multiplies
an independent uniform parameter. This gives a direct route to a `2^-128` AXU
proof. Permit zero parameter values; changing their distribution changes the proof.

For 0–15 bytes, `X1` is zero, so only one field multiply is needed. For 16–31
bytes, two are needed. Implement bounded loads or a small initialized scratch
array; never read past the slice to save an instruction. This is a different
construction from padding every tiny input into a bulk block.

The 31-byte threshold is part of the proposed algorithm. Changing a branch
threshold changes outputs unless the two branches are proven equivalent. Do not
choose different semantic thresholds on different CPUs.

### 3.3 Bulk compression: eight pairs plus independently keyed parity

For each 128-byte stripe, decode sixteen little-endian words. Pair the first
eight with the second eight:

```text
pair[i] = (word[i], word[i + 8])                  for i = 0..7
pair[8] = XOR of pair[0] through pair[7]          componentwise
v[i] = clmul64(pair[i].lo XOR key[i].lo,
               pair[i].hi XOR key[i].hi)          for i = 0..8
```

Every one of the nine pairs gets its own independent 128-bit key. In particular,
the parity pair has fresh key material. Any reduction in key material requires
a separate proof for that construction.

Each component has a target `2^-64` differential bound. Parity gives minimum
distance two. Interpret each `v[i]` as a field element, and combine with columns
`(1, c[i])` for the eight data pairs and `(0, 1)` for parity, where `c[i]` is the
field element with integer bit representation `i`:

```text
h0 = v[0] XOR ... XOR v[7]
h1 = v[8] XOR (c[0] * v[0]) XOR ... XOR (c[7] * v[7])
```

Every pair of columns is invertible: its determinant is either one or the
nonzero difference of two distinct coefficients. The intended consequence is a
joint `2^-128` AXU bound on the 256-bit pair `(h0, h1)`. This is a bound on the
pair, not 256-bit security and not a claim that each component independently has
128-bit security.

XOR-accumulate nine `v[i]` values over **64 stripes / 8 KiB**, using independent
keys at each stripe position. Apply the linear combination once per chunk.
Small constant field multiplications in that combination can be expressed with
XOR and field shifts. Document and test those identities before vectorizing.
Reuse the chunk's key table at corresponding positions in subsequent chunks;
the outer composition must justify that reuse.

Zero-pad only the last partial stripe. For equal message lengths, chunk and
stripe boundaries match. Bind different lengths independently in the outer hash.

### 3.4 Outer composition and length binding

Let each compressed chunk produce `(h0, h1)`. Draw independent uniform field
parameters `R`, `T`, and `S_long`, separate from every compression and short-path
parameter. Starting from `P = 0`, absorb each pair in message order:

```text
P = (P XOR h0) * R
P = (P XOR h1) * R
H_long(message) = P XOR (encode_u64(length) * T) XOR S_long
```

Use `H_short` for lengths at most 31, otherwise `H_long`. Independent branch
offsets provide a simple route to the cross-branch AXU argument. Merely adding
a public mode tag would not establish that argument.

Proof work to complete before any guarantee is advertised:

1. Prove the carryless component bound, parity distance, and matrix condition in
   the exact field representation above.
2. Prove that accumulation with independent keys at stripe positions preserves
   the joint chunk bound.
3. For two distinct equal-length messages of `q` chunks, choose a fixed chunk
   that differs. The probability that all compressed chunk pairs coincide is
   at most `2^-128`, even though keys are reused between chunks.
4. Otherwise the difference of outer polynomials is nonzero and has degree at
   most `2q`. Since `R` is independent, the root bound suggests total AXU error
   at most `(2q + 1) / 2^128`. Include arbitrary XOR targets, not just zero.
5. For different lengths within the long mode, the nonzero coefficient of the
   independent `T` gives a `2^-128` bound. Establish short-mode and cross-mode
   cases separately.
6. With `q <= ceil(L / 8192)`, verify
   `2q + 1 <= L / 4096 + 3`. Thus the proposed bound can meet the old advertised
   envelope. This is why 8 KiB is used here: using 4 KiB with this outer
   construction would not preserve that envelope automatically.
7. Cover empty input, partial stripes/chunks, length limits, and every parameter
   distribution. Write an independent formal model and proof, then match the
   optimized implementation to that model.

This is an initial mathematical argument to review, not a replacement for those
proof obligations. AES-expanded parameters additionally require a computational
assumption and explicit domain separation; a 128-bit seed cannot supply thousands
of information-theoretically independent random bytes.

### 3.5 Why it could be faster, and why it could lose

The existing inner compression uses ten carryless products per 128 bytes. This
candidate uses nine. The ideal multiplication-limited speedup is only `10/9`,
approximately **1.11×**, before other costs. The two outer field multiplies per
8 KiB match the frequency per byte of one per 4 KiB in the existing algorithm;
actual field reduction instruction counts still need measurement.

The candidate needs `64 * 9 * 16 = 9,216` bytes of compression keys, versus the
current 4,096-byte table. Including the six short/long field parameters gives
9,312 bytes of mathematical entropy, before alignment, schedules, or dispatch
metadata. It reads 144 key bytes per 128 message bytes. Key setup and many-key
cache behavior can therefore be worse. More SIMD lane work, parity reduction,
or spills can consume the entire arithmetic saving.

Test this tradeoff early. Do not shrink the table by reusing stripe keys without
a new proof. Even if the bulk candidate loses, the short-path and streaming
architecture experiments still identify useful requirements for another design.

## 4. Rust architecture and performance work

### API and contracts

Propose an immutable `PreparedKey`, a borrowed `Hasher<'key>`, and `Hash128` with
explicit little-endian serialization. Provide one-shot `hash128(&key, bytes)`
and `Hasher::update(&mut self, bytes) -> Result<(), LengthOverflow>` followed by
consuming `finish(self) -> Hash128`.

The required identity is:

```text
hash128(key, A || B || C) == finish(update(update(update(new(key), A), B), C))
```

Empty updates have no effect. Check total length before mutating the state and
reject messages exceeding `u64::MAX` bytes. Define reset and cloning explicitly.
Avoid a `std::hash::Hasher` adapter initially: it returns 64 bits and its typed
write conventions need a separately documented contract.

Start with an explicit-entropy constructor for proofs and tests. Add seed
expansion with labeled, disjoint derivation domains for bulk keys, short keys,
length parameters, offsets, and MAC keys. Keep the dependency-free core usable
in `no_std`. AES expansion and entropy acquisition can be optional features.

### Streaming state for the worked U8 candidate

Keep nine compression accumulators, the outer polynomial accumulator, total
length, chunk position, a tail of at most 127 pending bytes in a 128-byte array,
and the key/backend references. Set an initial U8 state-size budget of **512 bytes**;
measure `size_of` and stack usage rather than relying on a field-count estimate.

While total input is at most 31 bytes, preserve it for the short encoding. On
crossing that boundary, enter the long mode once and feed buffered bytes into
the bulk pipeline. Consume complete 128-byte stripes directly from each input
slice. Close the outer chunk at exactly 64 stripes, irrespective of update
boundaries. Retain only an incomplete stripe.

Finalization handles at most one padded stripe and one unfinished chunk, then
length binding. Invariants must prevent double absorption of a tail, skipped
bytes during the short/long transition, and an extra empty chunk at 8 KiB.
One-shot hashing should avoid creating or initializing the stream buffer.

### Backends

1. Write a small safe scalar oracle using explicit byte loads and fixed-loop
   carryless multiplication. Its job is clarity and correctness.
2. Add AArch64 PMULL and x86 PCLMUL kernels, with the same mathematical layout
   and output. Validate them against the oracle before optimizing scheduling.
3. Add AVX2+VPCLMUL and AVX-512+VPCLMUL variants. Dispatch on exact instruction
   requirements, not vector width alone. Retain a PCLMUL/SSE path where useful.
4. Compare unroll factors and numbers of live accumulators with disassembly and
   measurements. Count loads, shuffles, field reductions, spills, and dependency
   chains. Do not equate fewer source statements with fewer instructions.
5. Select a backend once per one-shot operation or hasher construction. Let a
   statically selected build use direct calls. Measure dynamic dispatch, first
   use, and short-message overhead separately.
6. Optimize the portable implementation only while preserving identical output.
   Investigate fixed-operation carryless multiplication using integer products
   or bitslicing, with independent derivation and tests. CPU timing assumptions
   also matter for keyed use; [BearSSL's discussion](https://bearssl.org/constanttime.html)
   is a useful starting point, not source to copy without its license.

Develop the NEON and AVX-512 kernels as primary implementations, with narrower
x86 paths providing coverage and useful short-input alternatives. The detailed
matrix in [SIMD.md](SIMD.md) covers EOR3/ternary XOR, lane layouts, wider PMULL,
AVX10, vector-length independence, polynomial folding, and batched AES work.

AVX-512 is a candidate, not a mandatory winner. Hardware-specific scheduling may
change; algorithmic lane assignment and digest bytes may not. Do not add manual
prefetch, non-temporal loads, or internal threading until a measured bottleneck
justifies them. Parallelizing independent messages is straightforward; changing
the within-message tree requires a new specification and proof.

Keep `unsafe` confined to load/store and intrinsic wrappers with explicit slice
length, alignment, initialization, and CPU-feature preconditions. Start with
ordinary initialized tail storage; use `MaybeUninit` only for a measured benefit.
Avoid secret-indexed tables and secret-dependent branches in keyed backends.

### MAC and avalanche behavior

A raw AXU result remains unsuitable as a public cryptographic checksum. Additional
mixing does not change that. Do not expose an avalanche variant until its exact
guarantee is specified: a fixed permutation preserves collision probability but
does not automatically preserve the AXU bound for every XOR target.

MAC implementation is a required milestone, exposed through an optional feature.
Independently implement a reviewed construction from the published
[nonce-based enhanced hash-then-mask analysis](https://eprint.iacr.org/2020/1145.pdf)
and derive its bound with this hash's actual epsilon. Use the current documented
contract as the minimum target: 128-bit tags, a 126-bit effective nonce space,
and the stated bounded nonce-reuse guarantee for messages up to 1 GiB
(`src/lib.rs:371`). Check the full theorem, truncation/domain details, all cipher
queries, and key-derivation assumptions before carrying over the numerical bound.

Specify effective nonce encoding explicitly, derive separate MAC keys, and use
constant-time tag verification. Keep raw-output and MAC key contexts separate so
a caller cannot reveal a MAC's secret universal hash through another API. A
nonce-unique-only wrapper would change the current contract and does not meet
this default. SIMD/AES scheduling may optimize an approved construction without
changing its nonce policy or authentication formula.

## 5. Implementation order and decision gates

| Phase | Deliverable | Gate before proceeding |
|---|---|---|
| 0. Establish requirements | CPU/input/update/key-reuse matrix; output/security/license contract; baseline data | Comparisons have matching output widths and labeled guarantees. |
| 1. Screen six families | Candidate-specific byte/parameter sheets, bound derivations, operation and memory budgets | Each survivor has a credible route to the required AXU and MAC contract. |
| 2. Scalar prototypes | Independent oracles, boundary vectors, streaming models for the first candidates | Proposed encodings and implementation invariants are consistent; no security claims yet. |
| 3. Hardware selection experiments | U16, M32, P256 plus bounded controls on NEON and AVX-512; narrower x86 baseline | Record end-to-end performance and memory; reject dominated designs. |
| 4. Complete the chosen proof and API | Full independent proof, compact stream state, one-shot paths, checked lengths, dispatch | The selected construction meets the actual bound and all backends agree. |
| 5. Tune measured bottlenecks | Targeted SIMD scheduling, portable optimization, key setup work | Gains survive repeated runs and real input/update distributions. |
| 6. Optional MAC feature | Reviewed construction, domain-separated keys, preserved nonce contract, verify API | Authentication claims match a checked theorem and tests. |
| 7. Release | Versioned vectors/specification, dependency/license inventory, published benchmark evidence | Correctness, proof, memory safety, and performance gates are met. |

Performance targets below are provisional acceptance thresholds, **not forecasts**:

- At least 25% lower geometric-mean latency over the 0–31-byte prepared-key
  bucket versus current raw output.
- At least 15% less time on selected fragmented-stream workloads representative
  of the application; publish each update-size result.
- At least 5% higher bulk throughput on each declared primary CPU family before
  claiming the new bulk algorithm is faster there. Require an uncertainty interval
  that excludes parity; a 5% point estimate alone is insufficient.
- No unexplained regression above 5% in any declared critical workload. Key
  creation and many-key workloads need explicit tradeoff decisions if they lose.
- Target 512 bytes of stream state for U8 and compact-buffer candidates, zero
  heap allocations in the hashing path, and measured/published prepared-key size.
  Wider-stripe or tree candidates must declare larger state requirements during
  selection; do not apply U8's estimate to them. Compare speed/state tradeoffs
  explicitly. P256 explores smaller keys; U8 does not promise them.

Reject a candidate that fails its stated workload before extensive API polishing.
Use the six-family comparison to choose which tradeoffs justify continued work.
Do not publish a general throughput claim merely because a small-input path wins.
Retain the confirmed universal/MAC requirements throughout selection.

## 6. Implementation layout

The implemented library lives in `crates/raijuhash/`, with its specification
and tests. The separate `benchmarks/` and `experiments/` packages, `proofs/`
project live at the repository root. Cargo build output
belongs under the ignored `target/` directory. The earlier proposed scratch
workspace was removed.

Keep field/encoding rules in shared, readable code. Duplicate hot loops only when
their measured instruction requirements differ. Do not build a generic hash
framework, publish multiple speculative variants, or add flags without a concrete
consumer. Pin the first stable digest definition and publish vectors before
allowing downstream users to persist it.

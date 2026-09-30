# Benchmarks

RaijuHash against other fast universal hashes on an Apple M1 and on an AMD
EPYC 9R45 (Zen 5), measured on 2026-09-29 using the `raijuhash` 0.1.0
source tree.

**Summary.** In the five-hash matrix, RaijuHash was the fastest on
Zen 5 at every input size from 16 bytes up, and on random lengths. Hashing a
large buffer on one core, it reached **133 GB/s**. That is 3.6× UMASH,
16× POLYVAL and GHASH, and 23× Poly1305. On the M1 it was the fastest
at 64 bytes and from 256 bytes up, at up to **68 GB/s**. There POLYVAL and
UMASH were up to 1.1 ns faster below 64 bytes, equal at 128 bytes, and UMASH
was faster on random lengths of 1–256 bytes. For messages longer than 16
bytes, RaijuHash also has the smallest proved bound of the XOR-universal
hashes compared (see [Contenders](#contenders)).

An x86-only extension below adds HalftimeHash16, authentication tags from
HMAC, GMAC, ChaCha20-Poly1305 and UMAC, plus SipHash-2-4 and HighwayHash64.
These separate suites answer different questions: raw hash-component speed,
complete tag-generation speed, and hash-table-style throughput. They do not
show that these primitives are interchangeable.

## Contenders

| Hash | Implementation | Output | Construction | Bound for two distinct messages of at most `L` bytes |
|---|---|---|---|---|
| **RaijuHash** | `raijuhash` 0.1.0 (this repository) | 128 bits | chain-coded carryless NH per 8 KiB chunk, polynomial over GF(2^128) | AXU: `(⌈L/8192⌉ + 1) / 2^128`, machine-checked in Lean for the reference implementation ([proofs](../proofs/README.md)) |
| POLYVAL | `polyval` 0.7.3 (RustCrypto) | 128 bits | polynomial evaluation in GF(2^128), as in AES-GCM-SIV | AXU: `(⌈L/16⌉ + 1) / 2^128`, with the length block added here |
| GHASH | `ghash` 0.6.0 (RustCrypto) | 128 bits | polynomial evaluation in GF(2^128), as in AES-GCM | as POLYVAL |
| Poly1305 | `poly1305` 0.9.1 (RustCrypto), `compute_unpadded` | 128 bits | polynomial evaluation modulo 2^130 − 5 | Δ-universal: `8⌈L/16⌉ / 2^106` |
| UMASH-128 | `umash-sys` 1.0.0 (the C library), `umash_fprint` | 128 bits (two 64-bit hashes) | carryless OH block compression, polynomial hash modulo 2^61 − 1 | almost-universal (collisions only, not XOR differences): `⌈L/2^26⌉² · 2^-83`, as stated in `umash.h` |

For a 1 MiB message these bounds are about 2^-121 (RaijuHash),
2^-112 (POLYVAL, GHASH), 2^-87 (Poly1305) and 2^-83 (UMASH,
collisions only). RaijuHash's bound grows by one step per 8 KiB chunk
instead of per 16-byte block. A MAC needs an XOR-universal hash (or a
Δ-universal one for Poly1305's addition), so UMASH is a hash-table and
fingerprinting reference point here, not a MAC building block.

POLYVAL and GHASH only hash whole 16-byte blocks. Each message is followed by
a 16-byte length block, as AES-GCM and AES-GCM-SIV do, so that they hash
messages of any length. That costs one block more per message. Poly1305 is
called as the complete Poly1305 function from a 32-byte key, including its
final addition. UMASH is built the way its crate packages it: C at `-O2`, on
x86 with `-mpclmul`, and without its optional long-input routines. For the
native x86 build the C code also got `-march=native`.

## Machines and method

| | Apple M1 | AMD EPYC 9R45 (Zen 5) |
|---|---|---|
| System | 4 performance + 4 efficiency cores, 16 GB, macOS 27.0 | AWS `c8a.large`: 2 cores, L2 1 MiB per core, L3 8 MiB visible to the VM; Linux 7.0 |
| Compilers | Rust 1.98.1, Apple clang | Rust 1.98.1, GCC 15.2 |
| Builds | default target (its baseline CPU has NEON, PMULL, AES and SHA3) | **native**: `-C target-cpu=native`; **generic**: no flags, CPU features detected at run time |
| RaijuHash backend | `NeonEor3` | `X86Avx512` |
| Pinning | none (not available on macOS); 0.5 s warm-up | `taskset -c 1` |

The harness is [`benchmarks/src/axu.rs`](../benchmarks/src/axu.rs). It works
as follows:

- Each call hashes one message through the public API. Inputs and outputs
  pass through `std::hint::black_box`. Keys are prepared outside the timer.
- Every hash is called through its own non-inlined function, so the code
  around each call is the same.
- At each size the hashes are timed in turn. A figure is the best of 3
  passes of 5 batches, and each batch runs for about 20 ms.
- Buffers are 64-byte aligned. The same buffer is hashed repeatedly, so
  inputs up to 1 MiB come from the L2 cache, 16 MiB from L3 or memory, and
  64 MiB from memory.
- Each machine ran the harness twice, and the tables give the better of the
  two runs. On the M1 every figure of the two runs agreed within 2.5%. On
  Zen 5, the retained figures agreed within 3%, except RaijuHash at 1 MiB
  (6%) and UMASH's key setup (5%).
- The random-length rows hash 2^16 messages whose lengths and offsets (in a
  64 KiB window) are drawn uniformly, so the branch predictor cannot learn
  the lengths.

**Bold** marks the fastest hash in a row. Each case was measured on one
machine only, so differences of a few percent are within noise.

## Apple M1

Short messages, ns per message:

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 8 B | 4.7 | 8.1 | 9.1 | 17.3 | **3.6** |
| 16 B | 5.0 | **4.6** | 5.6 | 15.6 | 5.9 |
| 32 B | 5.9 | **5.2** | 6.6 | 27.0 | 7.7 |
| 64 B | **5.9** | 7.7 | 9.1 | 54.3 | 8.2 |
| 128 B | 10.0 | **9.8** | 11.7 | 109.6 | 10.0 |
| 256 B | **11.9** | 15.6 | 18.3 | 219.3 | 15.0 |
| 512 B | **16.3** | 32.9 | 38.6 | 436.7 | 25.6 |

Longer messages, GB/s:

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 1 KiB | **36.4** | 14.1 | 13.2 | 1.2 | 22.8 |
| 4 KiB | **53.8** | 10.6 | 10.4 | 1.2 | 25.3 |
| 16 KiB | **64.8** | 9.7 | 9.7 | 1.2 | 26.1 |
| 64 KiB | **67.8** | 9.6 | 9.5 | 1.2 | 26.3 |
| 256 KiB | **57.4** | 9.5 | 9.5 | 1.2 | 26.4 |
| 1 MiB | **56.7** | 9.5 | 9.4 | 1.2 | 26.2 |
| 16 MiB | **54.3** | 9.4 | 9.4 | 1.2 | 25.5 |
| 64 MiB | **54.4** | 9.5 | 9.4 | 1.2 | 25.7 |

RustCrypto's `poly1305` has no NEON backend, which is why it runs at
1.2 GB/s here.

## AMD EPYC 9R45 (Zen 5), native build

Short messages, ns per message:

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 8 B | **3.3** | 15.7 | 16.7 | 69.8 | 3.8 |
| 16 B | **3.3** | 5.7 | 6.0 | 31.7 | 6.0 |
| 32 B | **4.4** | 7.3 | 7.9 | 34.9 | 8.6 |
| 64 B | **4.4** | 10.6 | 10.9 | 78.3 | 10.4 |
| 128 B | **5.9** | 18.2 | 18.7 | 90.6 | 11.6 |
| 256 B | **7.3** | 33.3 | 33.7 | 112.3 | 14.9 |
| 512 B | **9.8** | 63.5 | 63.9 | 156.4 | 27.3 |

Longer messages, GB/s:

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 1 KiB | **74.7** | 8.3 | 8.2 | 4.2 | 30.0 |
| 4 KiB | **108.6** | 8.4 | 8.4 | 5.3 | 35.2 |
| 16 KiB | **125.8** | 8.5 | 8.5 | 5.7 | 36.7 |
| 64 KiB | **132.4** | 8.5 | 8.5 | 5.8 | 36.8 |
| 256 KiB | **133.1** | 8.5 | 8.5 | 5.8 | 37.1 |
| 1 MiB | **126.4** | 8.5 | 8.5 | 5.8 | 37.0 |
| 16 MiB | **101.4** | 8.5 | 8.5 | 5.8 | 36.9 |
| 64 MiB | **54.3** | 8.4 | 8.4 | 5.8 | 35.1 |

On Zen 5, `vpclmulqdq` issues every other cycle at any vector width.
RaijuHash needs 8.125 64-bit carryless products per 128 bytes, which caps it
near 141 GB/s at this clock; it reaches 133. From memory (64 MiB) every
carryless hash is limited by one core's memory bandwidth.

## AMD EPYC 9R45 (Zen 5), generic build

This is what a program gets without `-C target-cpu=native`, the usual case
for a crate from crates.io. RaijuHash detects AVX-512 at run time, so it
changes little: short messages cost up to 0.5 ns more, and some are faster.
The other libraries lose more.

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 8 B (ns) | 3.8 | 26.5 | 34.3 | 373.1 | **3.5** |
| 16 B (ns) | **3.8** | 17.4 | 21.4 | 366.2 | 5.9 |
| 64 B (ns) | **4.0** | 18.3 | 23.7 | 637.4 | 9.2 |
| 256 B (ns) | **7.6** | 36.8 | 47.5 | 675.0 | 14.7 |
| 1 KiB (GB/s) | **81.9** | 7.6 | 6.0 | 1.2 | 29.8 |
| 64 KiB (GB/s) | **129.7** | 7.1 | 6.3 | 4.5 | 38.0 |
| 256 KiB (GB/s) | **132.6** | 7.1 | 6.3 | 4.7 | 38.3 |
| 16 MiB (GB/s) | **103.1** | 7.1 | 6.2 | 4.7 | 38.0 |

In this build, Poly1305 takes about 0.37 µs even for an 8-byte message, and
260 ns to set up a key.

## Random lengths

ns per message, lengths drawn uniformly from the range:

| Lengths | Machine | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---|---|---:|---:|---:|---:|---:|
| 1–64 B | M1 | **11.2** | 22.1 | 23.1 | 41.7 | 13.2 |
| 1–256 B | M1 | 18.9 | 28.9 | 31.3 | 118.8 | **16.3** |
| 1–1024 B | M1 | **31.1** | 50.8 | 55.0 | 443.6 | 36.1 |
| 1–64 B | Zen 5 native | **8.2** | 21.0 | 21.3 | 69.8 | 14.7 |
| 1–256 B | Zen 5 native | **11.6** | 37.8 | 37.8 | 107.3 | 17.9 |
| 1–1024 B | Zen 5 native | **19.4** | 86.2 | 86.3 | 179.6 | 40.8 |
| 1–64 B | Zen 5 generic | **8.2** | 34.6 | 42.6 | 386.7 | 13.5 |
| 1–256 B | Zen 5 generic | **11.5** | 45.5 | 54.5 | 605.8 | 17.2 |
| 1–1024 B | Zen 5 generic | **17.8** | 93.3 | 114.8 | 738.6 | 39.5 |

Mixed lengths cost RaijuHash more than fixed ones, because its code takes
different paths at 32 and 64 bytes and per 128-byte block.

## Key setup

ns to build one key:

| Operation | M1 | Zen 5 native | Zen 5 generic |
|---|---:|---:|---:|
| RaijuHash `Key::from_seed` | 178 | 58 | 86 |
| POLYVAL `new` | 4 | 8 | 10 |
| GHASH `new` | 6 | 10 | 24 |
| Poly1305 `new` | 2 | 8 | 260 |
| UMASH `umash_params_derive` | 751 | 548 | 557 |

A RaijuHash key is 9,472 bytes on x86 and 9,216 bytes on ARM.
`Key::from_seed` runs AES-128 in counter mode over the
field elements and the first nine table rows. It generates the rest of the
table, once, on the first message longer than 1 KiB. Keys are therefore
meant to be reused, not built per message. POLYVAL, GHASH and Poly1305 keys
are one or two field elements, plus whatever powers each implementation
precomputes.

## x86 authentication and keyed-hash comparisons

These additional measurements were taken on the AMD EPYC 9R45 (Zen 5) listed
above, pinned to one core. The release build used `-C target-cpu=native`;
RaijuHash selected `X86Avx512`. Rust was 1.98.1. Figures are the best of 3
passes of 5 batches, with each batch calibrated to about 20 ms. They are
single-host results without confidence intervals.

### Complete tag generation

The [`macs` harness](../benchmarks/src/macs.rs) compares 128-bit tags. HMAC-
SHA256 is truncated to 128 bits and authenticates a fixed-width 16-byte nonce
followed by the message. Each other construction receives a monotonically
increasing nonce. GMAC and ChaCha20-Poly1305 authenticate the message as AAD
with an empty plaintext, so this measures the complete tag path without
payload encryption. Keys are prepared before timing; per-message state setup
or copying remains inside the measured call. It measures tag generation, not
verification.

Short inputs are ns/tag; large inputs are GB/s:

| Input | RaijuHash-Mac | HMAC-SHA256-128 | AES-128-GMAC | ChaCha20-Poly1305 tag-only | UMAC-AES-128 |
|---:|---:|---:|---:|---:|---:|
| 8 B (ns) | **7.1** | 102.2 | 21.8 | 229.5 | 7,347.9 |
| 64 B (ns) | **8.0** | 138.2 | 25.2 | 256.2 | 7,382.7 |
| 512 B (ns) | **14.9** | 349.2 | 202.8 | 336.4 | 7,784.2 |
| 64 KiB | **129.7** | 2.2 | 2.5 | 5.6 | 1.0 |
| 1 MiB | **119.9** | 2.2 | 2.5 | 5.6 | 1.1 |
| 16 MiB | **105.2** | 2.2 | 2.5 | 5.7 | 1.1 |

RaijuHash-Mac was faster than these particular implementations on this host.
This is evidence for considering it in a new, private authentication protocol
whose key and nonce rules are designed for it. It does not establish a safe
drop-in replacement for HMAC, GMAC, or ChaCha20-Poly1305: those have standard
formats, interoperable implementations, and independent review, while
RaijuHash-Mac is a separate authentication construction with no encryption.
The ChaCha20-Poly1305 row is an AEAD tag-only workload, not a benchmark of
standalone Poly1305 with a reused one-time key. UMAC is the `purecrypto`
0.8.5 implementation; this is not a general result for all UMAC code. VMAC
was not measured because this workspace has no VMAC implementation.

### Hash-table-style keyed throughput

The [`keyed` harness](../benchmarks/src/keyed.rs) compares the low 64 bits of
RaijuHash `hash_avalanche` with 64-bit SipHash-2-4 and HighwayHash outputs.
The same prepared key and fixed RaijuHash tweak are reused; this is a
throughput comparison, not a security-equivalence test.

| Input | RaijuHash avalanche (ns/hash or GB/s) | SipHash-2-4 | HighwayHash64 |
|---:|---:|---:|---:|
| 8 B (ns) | **5.7** | 7.7 | 27.9 |
| 64 B (ns) | **6.7** | 21.8 | 23.3 |
| 256 B (ns) | **11.2** | 69.9 | 26.6 |
| 64 KiB | **130.9** | 4.0 | 12.1 |
| 1 MiB | **124.1** | 4.0 | 12.1 |
| 16 MiB | **111.3** | 4.0 | 12.1 |
| 64 MiB | **54.4** | 4.0 | 12.0 |

These figures do not justify using RaijuHash as a `HashMap` hasher for
attacker-controlled keys. The crate documents that `hash_avalanche` must not
be exposed to an adversary who chooses messages; use `Mac` for that exposure.
SipHash is the relevant keyed hash-table reference here. HighwayHash is only
the implementation measured. A fast table-hash result does not override the
different security properties.

### Universal-hash component extension

The fresh x86 run of [`axu`](../benchmarks/src/axu.rs) adds HalftimeHash16 to
the original raw component comparison. It hashes one message per call. The
table gives GB/s; the 64-bit UMASH result is a fingerprinting reference, not
an AXU MAC component. Poly1305 uses a fixed one-time key here as a primitive
microbenchmark, so those raw figures are not a safe repeated-tag use.

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 component | UMASH-128 | HalftimeHash16 |
|---:|---:|---:|---:|---:|---:|---:|
| 256 KiB | **132.8** | 8.5 | 8.5 | 5.8 | 37.0 | 88.0 |
| 1 MiB | **126.8** | 8.5 | 8.4 | 5.8 | 36.9 | 107.5 |
| 16 MiB | **108.7** | 8.4 | 8.4 | 5.8 | 36.8 | 89.6 |
| 64 MiB | **53.7** | 8.4 | 8.4 | 5.8 | 34.5 | 23.4 |

HalftimeHash16 is called through `digest_master_key`, which performs its
master-key setup on each one-shot call; its small-message timings therefore
include that work. The other component keys are prepared before timing. It is
an informational implementation comparison, not an equal-key-setup contest
or a standalone MAC comparison.

### What the measurements support

- **HMAC, GMAC, UMAC, or an AEAD authentication path:** RaijuHash-Mac is a
  candidate for a new custom authentication-only design when both endpoints
  can adopt it and its key/nonce rules. The measurements do not make it a
  compatible replacement for standardized protocols or full encryption.
- **GHASH or POLYVAL:** these can only be reconsidered as components in a
  redesigned and revalidated construction. RaijuHash is not a drop-in change
  to AES-GCM or AES-GCM-SIV.
- **SipHash or HighwayHash in a hash table:** the benchmark measures speed,
  but `hash_avalanche` is explicitly unsuitable for chosen-message exposure.
  It should not replace a keyed table hasher for adversarial keys.
- **SHA-256/BLAKE3 content IDs, signatures, or Merkle proofs:** not supported
  by these results. RaijuHash is keyed and almost-universal, not a public
  collision-resistant hash or signature primitive. Its algebraic combination
  can help parallelize known, indexed pieces, but a receiver still needs
  authenticated metadata and a way to detect missing or duplicated pieces;
  one final keyed tag does not provide public per-chunk proofs.

The reported harnesses can be rerun on the x86 host with:

```sh
RUSTFLAGS='-C target-cpu=native' CFLAGS='-O2 -march=native' \
  taskset -c 1 cargo run --release -p benchmarks --bin macs
RUSTFLAGS='-C target-cpu=native' CFLAGS='-O2 -march=native' \
  taskset -c 1 cargo run --release -p benchmarks --bin keyed
RUSTFLAGS='-C target-cpu=native' CFLAGS='-O2 -march=native' \
  taskset -c 1 cargo run --release -p benchmarks --bin axu
```

## RaijuHash in detail

**Streaming and latency.** The table uses `lat` and `quick` from
[`experiments/`](../experiments/src/bin/) and [`benchmarks/`](../benchmarks/src/).

- *One-shot* is `Key::hash`.
- *Streamed* is `hasher()`, one `update` and `finalize`.
- *Latency* writes the first byte of the message and then hashes it, so the
  hash's loads cannot forward from that store.
- *MAC* is `Mac::tag`: the hash, then two AES-128 encryptions, one of which
  waits for the hash.

All figures are in ns.

| Input | M1 one-shot | M1 streamed | M1 latency | M1 MAC | Zen 5 one-shot | Zen 5 streamed | Zen 5 latency | Zen 5 MAC |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 16 B | 4.0 | 4.9 | 10.8 | 7.7 | 2.7 | 3.8 | 9.7 | 5.6 |
| 64 B | 4.6 | 6.9 | 9.2 | 8.5 | 3.6 | 5.4 | 11.6 | 7.2 |
| 256 B | 10.4 | 14.3 | 16.0 | 16.5 | 6.8 | 9.3 | 18.3 | 12.5 |
| 1 KiB | 26.9 | 41.0 | 29.3 | 39.4 | 12.0 | 18.1 | 19.3 | 17.7 |
| 4 KiB | 76.7 | 79.2 | 76.5 | 92.4 | 37.6 | 43.3 | 37.0 | 47.7 |

These one-shot figures differ from the comparison tables by up to 0.7 ns.
Here the harness calls `Key::hash` inline; there it goes through a
non-inlined function, as every contender does.

**MAC latency** (time from the message to the tag): about 13 ns on Zen 5
and 16 ns on the M1 for messages up to 256 bytes. `Mac::from_seed` takes
71 ns on Zen 5 and 201 ns on the M1.

**Hasher state:** 1,536 bytes.

**Other backends on Zen 5.** Forcing a backend with `Key::with_backend`
measured these one-shot throughputs:
- AVX2: 67 GB/s at 64 KiB;
- SSE4.1: 34 GB/s;
- the portable code: 4.5 GB/s.

A CPU that has only AVX2 or SSE has a different carryless-multiply speed,
so these show the code paths, not the speed on such a CPU.

## Known limitation

With AVX-512, and with AVX2 or SSE for inputs of 1–31 bytes, the last
partial vector of a message is read with masked loads. On Zen 5 a masked
load takes a microcode assist of about 120–150 ns if its masked-off bytes
fall into the next page and that page is **not accessible or not yet
touched**. This happens when a message ends within 128 bytes of the end of
a memory mapping, or just before untouched pages of a fresh allocation. It
happens on every call for such a buffer, and the result is correct. Present
pages cost nothing extra, and none of the measurements above hit this case.

## Not covered

- Intel CPUs, and CPUs with only AVX2 or only PCLMULQDQ.
- Other operating systems.
- Several threads: see `Key::hash_parallel` and `benchmarks/src/par.rs`.
- Repeated runs with confidence intervals.

## Reproducing

```sh
# Apple M1 (default target)
cargo run --release -p benchmarks --bin axu

# x86-64, native and generic builds
RUSTFLAGS='-C target-cpu=native' CFLAGS='-O2 -march=native' \
  taskset -c 1 cargo run --release -p benchmarks --bin axu
taskset -c 1 cargo run --release -p benchmarks --bin axu

# RaijuHash detail
cargo run --release -p experiments --bin lat -- 16 64 256 1024 4096
```

The `benchmarks` package builds from this workspace and crates.io dependencies.
Its `axu`, `macs`, `keyed`, `quick`, `one`, and `par` binaries measure complete
public-API operations with `std::hint::black_box`; kernel and partial-operation
probes remain in the separate `experiments` package.

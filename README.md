# RaijuHash

[![crates.io](https://img.shields.io/crates/v/raijuhash.svg)](https://crates.io/crates/raijuhash)
[![docs.rs](https://docs.rs/raijuhash/badge.svg)](https://docs.rs/raijuhash)

RaijuHash is a fast keyed 128-bit hash for Rust. It is **almost-XOR-universal
(AXU)**, which makes it a building block for message authentication,
integrity checks and hash tables that resist hash flooding. The crate also
provides a nonce-based MAC built on it.

- **Proved bound.** For a uniformly random key, two distinct messages of at
  most `L` bytes have any fixed output XOR with probability at most
  `(⌈L/8192⌉ + 1) / 2^128`. This is machine-checked in Lean for the
  reference implementation.
- **Fast on every size.** One core hashes up to 133 GB/s on AMD Zen 5 with
  AVX-512, and a 16-byte message takes 3.3 ns.
- **Portable.** It has kernels for x86-64 (AVX-512, AVX2, SSE4.1) and
  AArch64 (NEON), picks one at run time, and falls back to portable Rust
  elsewhere. It supports `no_std` and needs no allocator.

RaijuHash is **not** a cryptographic hash or a public checksum. Its outputs
reveal information about the key, so they must stay secret. To send
something an adversary can see, use `Mac`.

## Usage

```toml
[dependencies]
raijuhash = "0.1"
```

```rust
use raijuhash::{Key, Mac};

// A key from a 128-bit secret seed (expanded with AES-128). Keys are about
// 9 KB and meant to be reused.
let key = Key::from_seed(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

// One-shot.
let h: u128 = key.hash(b"hello world");

// Incremental: the same result as hashing the concatenation.
let mut hasher = key.hasher();
hasher.update(b"hello ");
hasher.update(b"world");
assert_eq!(hasher.finalize(), h);

// Authentication: a 127-bit nonce that never repeats under one key.
let mac = Mac::from_seed(0xfedc_ba98_7654_3210_fedc_ba98_7654_3210);
let nonce = 1u128;
let tag = mac.tag(nonce, b"payload");
assert!(mac.verify(nonce, b"payload", tag));
```

The API at a glance:

| Item | Purpose |
|---|---|
| `Key::from_seed(u128)` | Key from a secret seed, with AES-128 in counter mode (feature `aes`) |
| `Key::from_entropy(&[u8; KEY_BYTES])` | Key from 8,432 uniformly random bytes |
| `Key::hash`, `Key::hasher` | One-shot and incremental hashing (`update`, `finalize`, `reset`) |
| `Key::hash_avalanche`, `Hasher::finalize_avalanche` | Output mixed with a 64-bit tweak, for hash tables; same collisions as `hash` (except for a `2^-128` fraction of keys) |
| `Key::hash_parallel` | One large message on several threads, same result (feature `std`) |
| `Mac::from_seed`, `Mac::tag`, `Mac::verify`, `Mac::hasher` | Nonce-based MAC (feature `aes`) |
| `Backend`, `Key::with_backend` | Report or force the kernel |

## Performance

Measured on 2026-09-29 on one core of an AMD EPYC 9R45 (Zen 5), with
RaijuHash's `X86Avx512` backend, Rust 1.98.1 and `-C target-cpu=native`.
Every hash is called through a non-inlined function, with inputs and outputs
passed through `std::hint::black_box`. Keys are prepared
before timing, except for HalftimeHash16 as noted below. **Bold** marks the
fastest measured implementation in each row.

Short messages, ns per hash (lower is better):

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---:|---:|---:|---:|---:|---:|
| 8 B | **3.3** | 15.7 | 16.7 | 69.8 | 3.8 |
| 16 B | **3.3** | 5.7 | 6.0 | 31.7 | 6.0 |
| 32 B | **4.4** | 7.3 | 7.9 | 34.9 | 8.6 |
| 64 B | **4.4** | 10.6 | 10.9 | 78.3 | 10.4 |
| 128 B | **5.9** | 18.2 | 18.7 | 90.6 | 11.6 |
| 256 B | **7.3** | 33.3 | 33.7 | 112.3 | 14.9 |
| 512 B | **9.8** | 63.5 | 63.9 | 156.4 | 27.3 |

Longer messages, GB/s (higher is better):

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

Random lengths drawn uniformly from each range, ns per hash (lower is better):

| Lengths | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 |
|---|---:|---:|---:|---:|---:|
| 1–64 B | **8.2** | 21.0 | 21.3 | 69.8 | 14.7 |
| 1–256 B | **11.6** | 37.8 | 37.8 | 107.3 | 17.9 |
| 1–1024 B | **19.4** | 86.2 | 86.3 | 179.6 | 40.8 |

An additional run on the same AVX-512 host includes HalftimeHash16, GB/s
(higher is better):

| Input | RaijuHash | POLYVAL | GHASH | Poly1305 | UMASH-128 | HalftimeHash16 |
|---:|---:|---:|---:|---:|---:|---:|
| 256 KiB | **132.8** | 8.5 | 8.5 | 5.8 | 37.0 | 88.0 |
| 1 MiB | **126.8** | 8.5 | 8.4 | 5.8 | 36.9 | 107.5 |
| 16 MiB | **108.7** | 8.4 | 8.4 | 5.8 | 36.8 | 89.6 |
| 64 MiB | **53.7** | 8.4 | 8.4 | 5.8 | 34.5 | 23.4 |

HalftimeHash16's one-shot `digest_master_key` API includes master-key setup
on each call. These tables measure raw hash components; Poly1305's fixed
one-time key is reused only for this microbenchmark. POLYVAL, GHASH and
Poly1305 are the RustCrypto crates, and UMASH is the C library.
[BENCHMARKS.md](https://github.com/raijuhash/raijuhash/blob/main/docs/BENCHMARKS.md)
documents the method, key setup, streaming, latency and the bound each hash
guarantees. The fixed-size buffers are hashed repeatedly; at 64 MiB,
carryless hashes are limited by one core's memory bandwidth on this host.

## Security

- **Hash.** RaijuHash is AXU with bound `(⌈L/8192⌉ + 1) / 2^128`. That is
  the same as POLYVAL's for one block, and 512 times smaller per byte for
  long messages. [SPEC.md](https://github.com/raijuhash/raijuhash/blob/main/crates/raijuhash/SPEC.md)
  defines the function and proves the bound.
- **Proof.** The bound is proved in Lean 4 for the mathematical function and
  for a transcription of the crate's reference implementation
  (`src/reference.rs`). The proofs contain no `sorry`, and the build fails on
  any axiom beyond Lean's standard three. The trust boundary is stated in
  [proofs/README.md](https://github.com/raijuhash/raijuhash/blob/main/proofs/README.md):
  - the transcription was checked by review and by frozen test vectors;
  - the optimized kernels are tested against the reference, not proved;
  - `Key::from_seed` additionally assumes that AES-128 is a pseudorandom
    permutation.
- **MAC.** `Mac` is nEHtM (Dutta–Nandi–Talnikar) over the hash truncated
  to 127 bits, masked with AES-128:
  `T = AES_K(0 || N) ⊕ AES_K(1 || (N ⊕ H(M)))`. With unique nonces the
  forging probability is at most `(2v + 29q + 12) · δ`, where
  `δ = (⌈L/8192⌉ + 1) / 2^127`, plus AES's PRP advantage. Keep each key to
  messages of at most 2^32 bytes, at most 2^48 tags and 2^48 failed
  verifications, and never repeat a nonce. SPEC §6 tabulates the bounds,
  including for repeated nonces.
- **Timing.** Branches and memory accesses depend only on message length
  and position, never on the key or the message contents. Tags are compared
  in constant time (`subtle`).
- **Never** expose raw hash values to an adversary who chooses the messages.

## Features and platforms

| Feature | Default | Enables |
|---|---|---|
| `std` | yes | run-time CPU detection, `Key::hash_parallel` |
| `aes` | yes | `Key::from_seed` and `Mac` (dependencies `aes`, `subtle`) |

With `default-features = false` the crate is `no_std` and needs no
allocator. The kernel is then chosen from compile-time target features, for
example with `-C target-cpu=native`.

| Target | Kernel |
|---|---|
| x86-64 with AVX-512 (F, BW, VL, VBMI2) and VPCLMULQDQ | AVX-512; AES-NI and VAES for key setup and the MAC |
| x86-64 with AVX2 and VPCLMULQDQ | AVX2 |
| x86-64 with SSE4.1 and PCLMULQDQ | SSE |
| AArch64 (little-endian) with NEON and PMULL | NEON, using `EOR3` where SHA3 is present, and ARMv8 AES |
| anything else | portable Rust, about 1 byte per cycle |

**Known limitation.** With AVX-512 (and AVX2/SSE at 1–31 bytes), the last
partial vector of a message is read with masked loads. On AMD Zen 5 these
take a microcode assist of about 120–150 ns if the message ends within
128 bytes of a page whose successor is unmapped or not yet touched. The
result is still correct. See
[BENCHMARKS.md](https://github.com/raijuhash/raijuhash/blob/main/docs/BENCHMARKS.md#known-limitation).

## How it works

Each 128-byte block is XORed with its predecessor and a key row. That
chain code makes any change touch at least two positions. Each 64-bit lane
pair is then multiplied carrylessly (carryless NH). The products are summed
per position over an 8 KiB chunk into two field elements, and chunks are
combined by a polynomial over GF(2^128). Messages under 32 bytes skip the
chunk code: they are one or two field multiplications. Messages of 32–64
bytes use precomputed coefficients. The bulk rate needs 8.125 64-bit
carryless products per 128 bytes.

## Repository

| Path | Contents |
|---|---|
| `crates/raijuhash/` | the published crate, its [specification](https://github.com/raijuhash/raijuhash/blob/main/crates/raijuhash/SPEC.md) and tests |
| `proofs/` | the Lean 4 proofs |
| `docs/BENCHMARKS.md` | measurements against other universal hashes |
| `docs/research/` | design notes and the history of the optimizations |
| `benchmarks/`, `experiments/` | harnesses and prototypes (not published) |

The design is clean-room and built only from published work:
- encode-hash-combine: Nandi, FSE 2014;
- carryless NH: Lemire and Kaser, arXiv:1503.03465, after NH in UMAC (RFC 4418);
- HalftimeHash: J. Apple, arXiv:2104.08865;
- the nEHtM MAC: Dutta, Nandi and Talnikar, EUROCRYPT 2019, with the bound of Choi, Lee, Lee and Lee, ASIACRYPT 2020.

## License

Released into the public domain under [CC0 1.0](LICENSE-CC0). Alternatively,
at your option, licensed under the [Apache License 2.0](LICENSE-APACHE) or
the [Apache License 2.0 with LLVM Exception](LICENSE-APACHE-LLVM).

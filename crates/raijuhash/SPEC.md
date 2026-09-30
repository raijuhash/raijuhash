# RaijuHash specification (version 1)

RaijuHash is a keyed hash from byte strings of length `0 <= L < 2^64` to 128
bits. With a key drawn uniformly at random and independently of the messages,
it is almost-XOR-universal (AXU): for distinct messages `m != m'` of at most
`L` bytes and any `d`,

```text
Pr[H(m) XOR H(m') = d]  <=  (ceil(L / 8192) + 1) / 2^128.
```

The construction and this argument were written independently from
published ideas (encode-hash-combine, carryless NH, polynomial hashing),
cited at the end.

## 1. Notation

- `F = GF(2^128) = GF(2)[x] / (x^128 + x^7 + x^2 + x + 1)`. A field element
  is a little-endian 128-bit integer; bit `i` is the coefficient of `x^i`.
  `*` is multiplication in `F`; `+` and `XOR` coincide.
- `clmul(a, b)` for 64-bit `a, b` is their product in `GF(2)[x]`, a
  polynomial of degree at most 126, i.e. a 128-bit value (no reduction).
- A small integer `j` used as a field element means the polynomial whose
  coefficients are the bits of `j`; for `j < 64`, `j` has degree below 6.
- Words are little-endian `u64`s.

## 2. Key

Uniform independent parameters (8432 bytes):

| Name | Size | Use |
|---|---|---|
| `K[0..=64]` | 65 rows of 16 words | chain keys: row `j` keys encoded position `j`; words 0..8 the X row, 8..16 the Y row |
| `A`, `B` | field elements | short messages |
| `R`, `R2` | field elements | outer polynomial |
| `T` | field element | length |
| `S` | field element | output offset |
| `V` | field element | avalanche finalizer only (section 6.5) |

`Key::from_entropy` reads them in that order (the table row by row, then
`A, B, R, R2, T, S, V`). `Key::from_seed(seed)` generates the same bytes as
AES-128 with key `seed` in counter mode on blocks
`(0x6b65795f63313238 << 64) | i`, `i = 0, 1, ...` (little-endian); then the
bound additionally assumes AES-128 is a pseudorandom permutation. `V` was
added after the others, so the hash itself uses only the first 8416 bytes.

## 3. Short messages (`L < 32`)

- `L < 16`: `X0` = the message, zero-padded to 16 bytes, with byte 15 set to
  `L`. Output `X0 * A + S`.
- `16 <= L < 32`: `X0` = the first 16 bytes; `X1` = the remaining bytes,
  zero-padded to 16, with byte 15 set to `L`. Output `X0 * A + X1 * B + S`.

## 4. Long messages (`L >= 32`)

### 4.1 Layout

Split the message into chunks of 8192 bytes; only the last may be shorter
(1..8192 bytes). A chunk is split into 128-byte blocks, the last zero-padded;
a chunk has `n` blocks, `1 <= n <= 64`. Block `b` consists of 16 words
`w[b][0..16]`: the X row `w[b][0..8]` and the Y row `w[b][8..16]`. Lane `l`
(0..8) of block `b` is the pair `(w[b][l], w[b][8 + l])`.

### 4.2 Chunk compression

Set `w[-1] = w[n] = 0`. For each encoded position `j = 0..=n` and lane `l`:

```text
tx[j][l] = w[j-1][l]     XOR w[j][l]     XOR K[j][l]
ty[j][l] = w[j-1][8 + l] XOR w[j][8 + l] XOR K[j][8 + l]
S[j]     = XOR over l of clmul(tx[j][l], ty[j][l])
```

Each lane is a chain code: position `j` encodes the difference of blocks
`j - 1` and `j` (a message difference in one block touches two positions).
Combine with column `(1, j)` for `j < 64` and column `(0, 1)` for `j = 64`
(only reached when `n = 64`):

```text
h0 = XOR over j <= min(n, 63) of S[j]
h1 = (XOR over j <= min(n, 63) of j * S[j])  +  (S[64] if n = 64)
```

### 4.3 Outer polynomial and output

With `P_0 = 0` and chunk pairs `(h0_i, h1_i)` in order, `i = 1..q`:

```text
P_i = (P_{i-1} + h0_i) * R + h1_i * R2
H   = P_q + L * T + S         (L as a field element below 2^64)
```

## 5. Security argument

Throughout, the key is uniform and independent of the (fixed, distinct)
messages `m, m'` and target `d`.

**Lemma 1 (carryless NH).** For fixed 64-bit `(x, y) != (x', y')` and uniform
independent 64-bit `a, b`, and any 128-bit `d`:
`Pr[clmul(x^a, y^b) XOR clmul(x'^a, y'^b) = d] <= 2^-64`.

*Proof.* Let `u = x^a`, `v = y^b`, `dx = x^x'`, `dy = y^y'`. The difference is
`clmul(u, dy) XOR clmul(dx, v) XOR clmul(dx, dy)`. If `dy != 0`, fix `v`; the
map `u -> clmul(u, dy)` is injective on 64-bit `u` (`GF(2)[x]` has no zero
divisors), and `u` is uniform, so at most one `u` hits `d`. If `dy = 0` then
`dx != 0` and the same holds for `v`. ∎

**Lemma 2 (chain code).** For distinct equal-length chunks, some lane's
encoded pairs `(tx[j][l] ^ K, ty[j][l] ^ K)` (the key-free parts
`e_j = w[j-1] ^ w[j]` restricted to that lane) differ in at least two
positions.

*Proof.* In a lane where the blocks differ, let `d_b` be the per-block
difference, `d_{-1} = d_n = 0`. The encoded differences are
`e_j = d_{j-1} ^ d_j`. The sequence `d_{-1}, d_0, ..., d_n` starts and ends
at 0 and is not all zero, so it changes value at least twice. Equal lengths
give equal `n` and identical zero padding, so padding creates no difference.
∎

**Lemma 3 (columns).** Any two of the columns `(1, j)` for `j < 64` and
`(0, 1)` are linearly independent over `F`: determinants are `j' - j != 0`
or `1`.

**Proposition 4 (chunk AXU).** For distinct chunks of equal length and any
`(d0, d1)`, `Pr[(h0 ^ h0', h1 ^ h1') = (d0, d1)] <= 2^-128`.

*Proof.* `(h0, h1)` is the XOR over lanes `l` of `V * (NH_{j,l})_j`, where
`NH_{j,l} = clmul(tx[j][l], ty[j][l])` and `V` has the columns above. Each
`(j, l)` uses its own key words. Pick a lane where the chunks differ and
condition on the key words of all other lanes, which fixes their
contribution. In that lane, Lemma 2 gives two positions `j1 != j2` whose
encoded pairs differ; condition also on the key words of all other positions
of that lane. The remaining difference is
`col(j1) * D1 + col(j2) * D2 = c` for a fixed `c`, where `D1, D2` are the NH
differences at `j1, j2`. By Lemma 3 this determines `(D1, D2)` uniquely. They
depend on disjoint independent key words, so by Lemma 1 the probability is at
most `2^-64 * 2^-64`. ∎

Note that key rows are reused across chunks; the argument never needs
independence between chunks, only within the chunk it conditions on.

**Theorem 5.** For distinct messages of lengths at most `L` and any `d`,
`Pr[H(m) ^ H(m') = d] <= (ceil(L / 8192) + 1) / 2^128`.

*Proof.*

*Different lengths.* `T` is independent of everything else in both outputs,
and short outputs do not involve `T`. The difference is `Z + (L_m ^ L_m') * T`
(long vs long) or `Z + L * T` (long vs short, `L >= 32`), with `Z`
independent of `T` and a nonzero coefficient on `T`; since multiplication by
a nonzero element is a bijection, the probability is exactly `2^-128`.

*Both short, same or different length.* The encodings `(X0, X1)` are
injective on messages below 32 bytes (`X1 = 0` exactly when `L < 16`; byte 15
records the length). Distinct encodings give a nonzero coefficient on `A` or
`B`, so the difference is uniform: `2^-128`.

*Both long, equal length.* Both have `q = ceil(L / 8192)` chunks with equal
chunk lengths. Unrolling,
`P_q = XOR_i [ h0_i * R^(q-i+1)  +  h1_i * R2 * R^(q-i) ]`: a polynomial in
`(R, R2)` whose monomials are distinct and non-constant, of total degree at
most `q`. `R, R2` are independent of the chain keys. Condition on the chain
keys. If some chunk pair differs, `P_q - P'_q - d` is a nonzero polynomial of
degree at most `q` (for `d = 0` it has a nonzero non-constant coefficient;
otherwise a nonzero constant term), so by Schwartz–Zippel it vanishes with
probability at most `q / 2^128`. Otherwise all chunk pairs coincide, which
requires in particular the first differing chunk's pairs to collide:
probability at most `2^-128` by Proposition 4 (reused keys do not matter; we
only bound one event). Total `(q + 1) / 2^128`. ∎

**Remarks.**

- The optional MAC (`Mac`) is specified and bounded in section 6.
- Like every universal hash, outputs reveal information about the key; raw
  outputs must not be exposed to an adversary who chooses messages.
- The statement covers the mathematical function. Implementations are tested
  for agreement with `src/reference.rs`, a line-by-line transcription of this
  document. The [Lean verification project](https://github.com/raijuhash/raijuhash/blob/22d7797033f97d84bb9bb82e6166991c9d114eb0/proofs/README.md)
  machine-checks Theorem 5 for this field, and for a Lean transcription of
  `src/reference.rs` with uniformly random key bytes. The transcription is
  tied to the Rust code by review and the frozen vectors, and the optimized
  backends by tests; neither link is a proof.

## 6. MAC (`Mac`, feature `aes`)

### 6.1 Definition

A MAC key is a hash key `K_h` (the first 8416 bytes of section 2) and an
AES-128 key `K`, independent and uniform. `Mac::from_parts` accepts the full
8432-byte key encoding of section 2 and `K`; its `V` field does not affect
the MAC.

`Mac::from_seed(seed)` generates `K_h` with AES-128 under key `seed` in
counter mode on blocks `(0x6873685f63616d63 << 64) | i`, `i = 0..=525`, and
`K` as the encryption of `(0x7068635f63616d63 << 64) | 0`.
The implementation also derives `V` at counter 526 in the hash-key domain,
but never uses it for tagging or verification. This unobserved block can
be omitted from the security reduction.

Let `N` be the nonce reduced mod `2^127` and `h = H_{K_h}(M) mod 2^127`.
With blocks read as little-endian 128-bit integers, the tag is

```text
T = AES_K(N)  XOR  AES_K(2^127 + (N XOR h)).
```

Verification recomputes `T` and compares all 128 bits.

### 6.2 Bound

**Lemma 6 (truncation).** For messages of at most `L` bytes, `h` is
`delta`-AXU with `delta = (ceil(L / 8192) + 1) / 2^127`.

*Proof.* `h ^ h' = X (mod 2^127)` exactly when `H(M) ^ H(M')` is `X` or
`X + 2^127`. By Theorem 5 each has probability at most
`(ceil(L / 8192) + 1) / 2^128`. ∎

So `Mac` is nEHtM with `n = 128` over an `(n - 1)`-bit `delta`-AXU hash,
the setting of DNT19 (Theorem 1) and CLLL20 (Theorem 2). CLLL20 states that
setting at the start of its Section 4, and its proof uses the AXU property.
The theorem header's "`{0,1}^n`, `delta`-almost universal" does not match
that setting; the setting stated in Section 4 is used here.

The adversary makes `q` tag queries and `v` verification queries on messages
of at most `L` bytes. `mu` counts *faulty* tag queries: tag queries that use
a nonce an earlier tag query used with a different message. Verification
queries may repeat nonces freely. The forging probability is at most

```text
Adv_prp_AES(2(q + v)) + min(DNT19(q, v, mu, delta), CLLL20(q, v, mu, delta))
```

and, for `Mac::from_seed`, additionally
`Adv_prp_AES(527) + 527 * 526 / 2^129` (at most `2^-110.9` beyond the PRP
term). That term replaces AES under `seed` by a random permutation. Only
the 527 distinct counter blocks affecting the MAC are counted: 526 for
`K_h` and one for `K`, excluding the unused `V` block. These outputs are
within that statistical distance of independent uniform keys. If
`Key::from_seed` is used for raw hashes with the same seed, count its 526
blocks too: `1053 * 1052 / 2^129`, at most `2^-108.9`.
Raw hashes from that key then reveal nothing about the MAC keys beyond this
distance.

**Unique nonces.** With `mu = 0` and `q <= 2^64`, DNT19 Theorem 1 reads

```text
v delta + 4 q^3 delta / 2^128 + 12 q^4 delta / 2^256 + 48 q^3 / 2^256 + (q + 2v) / 2^128.
```

Use `q^2 <= 2^128` in the second, third and fourth terms, and
`1 / 2^128 <= delta / 2` (as `delta >= 2^-127`). This gives

```text
Adv_forge  <=  (2v + 29q + 12) * delta  +  Adv_prp_AES(2(q + v)).
```

**Concrete values.** The table shows the exact minimum of the two
information-theoretic bounds, without the AES terms. It was computed by
`gen/mac_bound.py`, with the free parameter of CLLL20 optimized.

| max message | tags `q` | verifications `v` | faulty `mu` | forging bound |
|---|---|---|---|---|
| 2^20 B | 2^48 | 2^48 | 0 | 2^-72.0 |
| 2^32 B | 2^48 | 2^48 | 0 | 2^-60.0 |
| 2^40 B | 2^48 | 2^48 | 0 | 2^-52.0 |
| 2^40 B | 2^64 | 2^64 | 0 | 2^-33.7 |
| 2^32 B | 2^64 | 2^48 | 0 | 2^-42.0 |
| 2^32 B | 2^64 | 2^48 | 2^16 | 2^-41.4 |
| 2^32 B | 2^64 | 2^48 | 2^32 | 2^-40.2 |
| 2^32 B | 2^64 | 2^48 | 2^48 | 2^-9.7 |
| 2^40 B | 2^80 | 2^48 | 0 | 2^-14.5 |
| 2^64 − 1 B | 2^48 | 2^48 | 0 | 2^-28.0 |

### 6.3 Usage limits

Recommended per key:

- messages of at most `2^32` bytes (`delta <= 2^-107.99`);
- at most `2^48` tags and `2^48` failed verifications (bound `2^-60`);
- nonces that never repeat. A 127-bit counter never wraps in practice.
  Nonces equal in their low 127 bits are the same nonce.

Beyond these limits, rekey. Repeated nonces degrade security gracefully:
`2^32` repeats among `2^64` tags still give `2^-40`. Near `2^48` repeats
the bound is lost, consistent with the forgery DNT19 gives at about
`2^(n/2)` faulty queries. At `2^80` tags the bound is weak whatever the
nonces. The length limit matters because every verification query
contributes about `delta`, which grows linearly with the message length.

### 6.4 Timing

- The hash branches and indexes memory by message length and position
  only, never by key or message contents. The SIMD backends use carryless
  multiply instructions. The portable backend uses `clmul32`, a masked
  integer multiplication that is constant-time wherever integer
  multiplication is. Key setup (`mul_x`) selects the reduction with a mask.
- AES uses the CPU's AES instructions directly (ARMv8 AES; AES-NI, with
  VAES for counter mode) where detected. Otherwise it comes from the `aes`
  crate's constant-time fixsliced software implementation.
- Tags are compared with `subtle::ConstantTimeEq` on `u128`. This was
  inspected in two release builds with Rust 1.98.1, and in both
  `Mac::verify` and `MacHasher::verify` pass the comparison bit through
  `subtle`'s out-of-line volatile barrier and only then convert it to
  `bool`.
  - **AArch64 (Apple M1):** the tag halves are compared with
    `cmp`/`ccmp`/`cset`.
  - **x86-64 (AMD EPYC 9R45):** the halves are XORed, ORed and set with
    `sete`.

  In neither build do they branch on the tag; their only branch is the
  public message-length test. This is a property of generated code, so
  re-inspect after changing toolchains.

### 6.5 Avalanche output

`Key::hash_avalanche(M, tweak)` and `Hasher::finalize_avalanche(tweak)`
return `mix(H(M), tweak) * V`. `mix` (`reference::avalanche_mix`) adds the
64-bit `tweak` to the low half, applies MurmurHash3's 64-bit finalizer
(public domain) to each half, then sets `lo ^= rotl(hi, 32)` and
`hi += lo` (wrapping). Every step is invertible, so for a fixed tweak `mix`
is a bijection.

For distinct messages and any `d`, with `V` uniform and independent of the
hash key: if `H(M) = H(M')`, the output difference is 0, which matches `d`
only for `d = 0`; that event has probability at most `epsilon`. Otherwise
`mix(H(M)) ^ mix(H(M'))` is nonzero and fixed given the hash key, and the
product with `V` equals `d` with probability `2^-128`. So the output is
`(epsilon + 2^-128)`-AXU; this count includes `V = 0`. If `V != 0`,
multiplication by `V` is injective, and collisions are exactly those of `H`
for every tweak. If `V = 0` (a `2^-128` fraction of keys; the key is not
checked for it), every output is 0. Like the raw hash, it must not be
exposed to an adversary who chooses messages.

## 7. Implementation notes (not part of the definition)

- `h1` is evaluated from position sums: with `j = 8u + v`, the SIMD kernels
  keep `B[v] = XOR_u S[8u+v]` and the planes `F[b] = XOR over u with bit b of
  C[u]`, `C[u] = XOR_v S[8u+v]`, and use
  `h1 = sum_{b<3} x^b (XOR_{v: bit b} B[v]) + sum_{b<3} x^(3+b) F[b] + S[64]`.
  For at most 8 blocks, `h0 R + h1 R2` is formed directly as
  `h0 R + sum_b P_b (x^b R2)` with precomputed `x^b R2`.
- Streaming state never buffers more than one 1 KiB group; whole groups are
  hashed straight from the input once more data follows them, so a message
  of at most one group is hashed in one pass by `finalize`.
- `Key::from_seed` generates the field parameters and table rows `0..=8` at
  construction and the remaining rows on the first message that needs them
  (all backends).
- A 32..=64-byte input is one block with a zero Y row, so its output is
  affine in the eight X words; the coefficients are precomputed from the key
  (`S2Coeffs` in `lib.rs`) and the output is identical. The NEON and all x86
  backends use this.
- NEON only: one-shot inputs of one group or less multiply each position
  sum `S_j` by a precomputed `R + j R2` as it is produced, instead of forming
  `h0, h1`; whole chunks are hashed two at a time, sharing each key-row load
  between them, and which of three kernels is used depends only on the
  input size.
- AVX-512 only (`docs/research/X86_PORT.md`):
  - sums stay per 128-bit lane until an output is formed;
  - `h0 R + h1 R2 + L T` is one packed product with `(R, T, R2, 0)`;
  - a partial last block is read with masked loads;
  - whole chunks go one at a time;
  - a streaming hash keeps only `h0` and `h1` (with the previous block)
    between calls.

## References

- M. Nandi, *On the minimum number of multiplications necessary for universal
  hash functions*, FSE 2014 / ePrint 2013/574 (encode-hash-combine).
- J. Apple, *HalftimeHash*, arXiv:2104.08865.
- D. Lemire, O. Kaser, *Faster 64-bit universal hashing using carry-less
  multiplications*, arXiv:1503.03465 (carryless NH).
- J. Black et al., *UMAC*, RFC 4418 (NH).
- A. Dutta, M. Nandi, S. Talnikar, *Beyond birthday bound secure MAC in faulty
  nonce model*, EUROCRYPT 2019, ePrint 2019/127 (nEHtM; "DNT19").
- W. Choi, B. Lee, Y. Lee, J. Lee, *Improved security analysis for nonce-based
  enhanced hash-then-mask MACs*, ASIACRYPT 2020, ePrint 2020/1145 ("CLLL20").

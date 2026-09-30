# Formal verification

The AXU bound of SPEC.md (Theorem 5) is machine-checked in Lean 4, for the
mathematical hash and for a Lean transcription of
`crates/raijuhash/src/reference.rs`, in Rust's field and bit layout. The
default build runs an axiom audit on every exported theorem: only
`propext`, `Classical.choice` and `Quot.sound` are allowed, so an admission
(`sorry`), a custom axiom or `native_decide` anywhere in a proof fails the
build.

Run from this directory with the pinned Lean 4.34.0 and mathlib versions:

```sh
lake build                    # all proofs and the axiom audit
python3 test_axiom_audit.py   # the audit must reject invalid proofs
python3 check_vectors.py      # Lean vectors match crates/raijuhash/tests/vectors.rs
```

Machine-checking stops at the Lean transcription of `reference.rs`. The
Rust code itself is linked to it by review and by frozen test vectors. See
[Trust boundary](#trust-boundary).

## What is proved

| Lean theorem (`RaijuHashCheck.lean`) | Statement |
|---|---|
| `nh_axu` | SPEC Lemma 1: carryless NH is `2⁻⁶⁴`-AU |
| `chain_distance_two` | SPEC Lemma 2: the chain code has distance ≥ 2 |
| `ChunkProof.columns_independent` | SPEC Lemma 3: any two columns are independent |
| `chunk_axu` | SPEC Proposition 4: one chunk is `2⁻¹²⁸`-AXU over the whole 65-row table |
| `raijuhash_axu` | SPEC Theorem 5 for any field of `2¹²⁸` elements and any injective bit layout |
| `fPoly_irreducible` | `X¹²⁸ + X⁷ + X² + X + 1` is irreducible over `𝔽₂` |
| `raijuhash_axu_gf128` | Theorem 5 in `GF128 = 𝔽₂[X]/(X¹²⁸ + X⁷ + X² + X + 1)` |
| `Reference.gfMul_spec` | `gf_mul` is multiplication in `GF128` and returns a `u128` |
| `Reference.chunkPair_spec` | `chunk_pair` computes the model's `(h0, h1)` |
| `Reference.hash_spec` | `reference::hash` computes the model's `raijuhash` |
| `Reference.reference_hash_axu` | Theorem 5 for `reference::hash` with uniformly random key bytes |
| `Reference.Vectors.pattern_*` | 11 frozen outputs of `tests/vectors.rs`, evaluated by the kernel |

The last theorem is the end-to-end statement. For uniformly random key bytes
`kb` (`KEY_BYTES = 8432`, parsed by the transcribed `Params::from_bytes`),
distinct byte strings `m ≠ m'` of at most `L < 2⁶⁴` bytes, and any `d`:

```text
#{kb | hash(kb, m) ^ hash(kb, m') = d} / 256^8432  ≤  ((L + 8191) / 8192 + 1) / 2^128
```

## Proof outline

**Model** (`RaijuHashCompress.lean`, `RaijuHashFull.lean`): SPEC sections
3–4 over a field `F` with an additive map `ι` from 128-bit polynomials.

**Proposition 4** (`ChunkProof/Main.lean`). `digest_diff_eq` writes the
digest difference as a sum over every position `j ≤ 64` and lane `l` of
`column(j) · NH_{j,l}`, where `NH_{j,l}` depends only on the table entry
`K[j][l]` and unused positions get the zero column. Lemma 2 gives two
positions in one lane whose key-free encoded pairs differ. `splitEquiv`
splits the table into those two entries and the rest. For each fixed rest,
the difference is a constant plus an invertible column mix of two
independent NH differences (Lemma 3), so at most `2¹²⁸` of the `2²⁵⁶` pairs
of entries hit the target (Lemma 1, twice).

**Theorem 5** (`FullProof/`). Different lengths and short messages use a
uniform key coordinate (`T`, `A` or `B`). For two long messages of equal
length (`FullProof/Long.lean`):

- `exists_chunk_diff`: some chunk index has chunks of equal size that differ
  in a block;
- `long_hash_diff`: the output difference is the formal outer polynomial of
  the per-chunk digest differences at `(R, R2)`; length and offset cancel;
- `outerPoly_eq_C`: that polynomial is a constant only if all chunk
  differences and the constant are zero;
- `card_both_long`: condition on the table. A nonzero event polynomial has
  total degree at most `q` and at most `q · 2¹²⁸` zeros (Schwartz–Zippel);
  an identically zero one forces the differing chunk to collide, which by
  Proposition 4 happens for at most `2⁻¹²⁸` of the tables.

**The field** (`Bits.lean`, `Frobenius.lean`, `Rabin.lean`, `GF128.lean`).
`irreducible_of_frobenius` is a Rabin test: a monic degree-128 polynomial
`f` over `𝔽₂` is irreducible if `f ∣ X^(2¹²⁸) - X` and `X^(2⁶⁴) - X` is
coprime to `f`. Both hypotheses are checked by the kernel on bit
representations: `frobChain` lists `X^(2^k) mod f` for `k = 1…128`, and
`squaringChain` checks each squaring step. A Bezout identity, whose
coefficients were computed offline, is also checked. The quotients used
during reduction are not trusted, because any quotient gives a valid
congruence.

**Rust reference** (`Reference.lean`, `ReferenceProof/`).
`Reference.lean` transcribes `reference.rs`: `clmul64`, `reduce256`,
`gf_mul`, `Params::from_bytes`, `chunk_pair` and `hash`. It keeps Rust's
integer semantics: `u128` shifts discard high bits, `as u64` and `as u8`
truncate, and `^`, `>>` and `&` are the same bit operations. The proofs show:

- `clmul64` is the polynomial product; the two folds of `reduce256` replace
  `X¹²⁸` by `X⁷ + X² + X + 1`. The second fold never overflows
  (`reduceFold_small`), which also proves the `debug_assert_eq!(over2, 0)`
  in `reduce256`.
- each `p_j` of `chunk_pair` is the model's `S[j]`, and `(h0, h1)` is the
  model digest;
- the chunk slices, short-path tagged blocks and outer loop of `hash` match
  `msgChunk`, `bytes128 … + len·X¹²⁰` and `accumR`;
- `splitKey` is a bijection from the 8432 key bytes to the model key and the
  16 bytes of `V`, which `hash` does not read. So uniform key bytes give a
  uniform model key.

## Trust boundary

These are **not** machine-checked:

1. **Transcription fidelity.** `RaijuHash/Reference.lean` was written by
   hand from `reference.rs` and compared with it by reading. There is no
   verified Rust semantics or extraction (Aeneas, hax and Charon are not
   installed here). Modelling choices: byte slices are lists of naturals
   below 256; `msg.chunks(CHUNK)` is written with index arithmetic; the
   `assert!` in `chunk_pair` is omitted because `hash` only passes chunks of
   1–8192 bytes; lengths are below `2⁶⁴`. The transcription reproduces all
   15 `PATTERN_KEY` vectors of `crates/raijuhash/tests/vectors.rs`
   (`ReferenceVectors.lean`, cross-checked by `check_vectors.py`). The Rust
   tests pin every backend to those vectors.
2. **Optimized code.** The portable, NEON, NEON-assembly and x86 backends,
   the streaming state and the `Key`/`Hasher` API are tested against
   `reference.rs` (`tests/agree.rs`, `tests/vectors.rs`), not proved.
3. **Key generation, MAC and avalanche.** The bound assumes independent
   uniform key bytes. `Key::from_seed` additionally relies on AES-128 being a
   PRP. The MAC bound (SPEC §6.2) uses published nEHtM results. The
   avalanche finalizer (§6.5) is not modelled.
4. **Evaluation of four long vectors.** The 8191-, 8192-, 8193- and
   20000-byte vectors are checked with `#guard` (compiled evaluation),
   because list indexing makes kernel evaluation slow. They are tests; no
   theorem depends on them. The other 11 vectors are kernel-checked theorems.
5. **The tools.** Lean's kernel, mathlib, and the `audit_axioms` command
   (`RaijuHash/AxiomAudit.lean`, tested by `test_axiom_audit.py`).

## History: problems fixed in the earlier model

| Problem | Correction |
| --- | --- |
| `KeyTable = ℕ → KeyRow` was infinite; `Nat.card` of an infinite type is zero, so dividing by it makes the claimed probabilities zero | Exactly 65 key rows (`Fin 65`); finiteness and positive key-space cardinalities are proved |
| `(j : F)` interpreted a position as a natural-number cast, giving only 0 or 1 in characteristic two | `bitPoly 6 j` encodes the six bits as polynomial coefficients; distinct columns are proved |
| `msgChunk` claimed a nonempty chunk for any index, including an empty message | Only `Fin (numChunks m)` indices are accepted; both constructor invariants are proved |
| Short encodings omitted length markers and loaded the wrong extent | Exactly 128 bits are loaded; total length is placed at bit 120 in the appropriate half |
| NH counting used an incorrect constant term and did not compile | Correct affine identity and complete finite counting proof |
| The standalone accumulator reversed chunk order | The accumulator uses the same forward `foldl` as Rust |
| The supposed Schwartz–Zippel lemma allowed arbitrary functions and uncontrolled identically-zero slices | Mathlib's theorem for nonzero formal bivariate polynomials and **total degree** is used |
| The final bound used rational `L / 8192` instead of a ceiling | The statement now uses natural-number `(L + 8191) / 8192` |
| Printing axioms did not fail the build | `audit_axioms` rejects unapproved transitive axioms |

`chunk_axu` no longer assumes `Fintype F` or `|F| = 2¹²⁸`. Its count uses
only the key words, so the statement is stronger than before.

## Maintenance notes

- Keep `2¹²⁸`-sized exponents abstract in lemma statements (`N = 2 ^ 128`),
  as in `fPoly_dvd_frobenius`. Otherwise the elaborator or the kernel may try
  to unfold a power with that exponent.
- `fPoly` is `irreducible` after `toPoly_fNat` for the same reason. Use
  `unfold fPoly` where its definition is needed.
- `decide +kernel` on literals is fast. Nesting unevaluated steps, such as
  `sqF^[128] 2`, is slow in the call-by-name kernel, so `frobChain`
  lists every intermediate value.
- Historical `scratch*.lean` files are not build targets.

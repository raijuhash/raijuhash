import RaijuHashVerified
import RaijuHash.NHProof.Main
import RaijuHash.ChainProof.Main
import RaijuHash.ChunkProof.Main
import RaijuHash.FullProof.Main
import RaijuHash.GF128
import RaijuHash.ReferenceProof.Axu
import RaijuHash.ReferenceVectors

/-!
# The theorems

The results about the definitions in `RaijuHashCompress.lean`,
`RaijuHashFull.lean` and `RaijuHash/Reference.lean`. The proofs are in
`RaijuHash/`; the default build rejects exported theorems with transitive
axioms other than `propext`, `Classical.choice`, and `Quot.sound`.

## Correspondence with SPEC.md and `reference.rs`

| Lean theorem | Statement |
|---|---|
| `nh_axu` | SPEC Lemma 1 (carryless NH) |
| `chain_distance_two` | SPEC Lemma 2 (chain code) |
| `ChunkProof.columns_independent` | SPEC Lemma 3 (columns) |
| `chunk_axu` | SPEC Proposition 4 (chunk AXU) |
| `raijuhash_axu` | SPEC Theorem 5, any `GF(2¹²⁸)` and bit layout |
| `fPoly_irreducible` | `X¹²⁸ + X⁷ + X² + X + 1` is irreducible over `𝔽₂` |
| `raijuhash_axu_gf128` | Theorem 5 in the field of `gf_mul` |
| `Reference.gfMul_spec` | `gf_mul` is multiplication in that field |
| `Reference.chunkPair_spec` | `chunk_pair` computes the chunk digest |
| `Reference.hash_spec` | `reference::hash` computes `raijuhash` |
| `Reference.reference_hash_axu` | Theorem 5 for `reference::hash` on random key bytes |

In `raijuhash_axu`, `ι` maps a polynomial of degree `< 128` to a field
element, and the bound holds for every such injective `ι`. The later
theorems fix Rust's field and bit layout.
-/

namespace RaijuHash

open Polynomial

/-- **Lemma 1 (carryless NH is `2⁻⁶⁴`-almost-universal).**

For distinct pairs `(x, y) ≠ (x', y')` of 64-bit words and uniform independent
64-bit keys `a, b`, the carryless product difference `clmul(x ⊕ a, y ⊕ b) ⊕
clmul(x' ⊕ a, y' ⊕ b)` equals any fixed 128-bit target `d` with probability
at most `2⁻⁶⁴`.

The proof follows SPEC.md Lemma 1: the difference is affine in the uniform key
word, and `GF(2)[x]` is an integral domain, so a nonzero coefficient makes the
map injective. -/
theorem nh_axu {M : Type} [AddCommGroup M]
    (ι : R →+ M) (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (x y x' y' : Word) (hne : (x, y) ≠ (x', y')) (d : M) :
    (Nat.card {ab : Word × Word //
      ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d} : ℚ)
      / Nat.card (Word × Word) ≤ 1 / 2 ^ 64 :=
  NHProof.nh_axu ι hι x y x' y' hne d

/-- **Lemma 2 (chain code has distance ≥ 2).**

For distinct equal-length chunks (same number of blocks, differing in at least
one block), there is a lane `l` in which the encoded pairs
`(tx[j][l], ty[j][l])` differ from `(tx'[j][l], ty'[j][l])` at two or more
positions `j`.

The proof follows SPEC.md Lemma 2: the encoded differences `e_j = d_{j-1} ⊕ d_j`
form a sequence that starts and ends at 0, and is not all zero, so it changes
value at least twice. -/
theorem chain_distance_two
    (c c' : Chunk) (hn : c.n = c'.n)
    (hdiff : ∃ b, b < c.n ∧ c.blocks b ≠ c'.blocks b)
    (K : KeyTable) :
    ∃ l : Lane, ∃ j₁ j₂ : ℕ, j₁ < j₂ ∧ j₂ ≤ c.n ∧
      encodedPair c K j₁ l ≠ encodedPair c' K j₁ l ∧
      encodedPair c K j₂ l ≠ encodedPair c' K j₂ l :=
  ChainProof.chain_distance_two c c' hn hdiff K

/-- **Proposition 4 (single chunk is `2⁻¹²⁸`-AXU).**

For distinct chunks of equal length and any target `(d₀, d₁)`, the probability
that the chunk digests satisfy `(h0 ⊕ h0', h1 ⊕ h1') = (d₀, d₁)` is at most
`2⁻¹²⁸`, over uniform independent key rows.

The proof follows SPEC.md Proposition 4: Lemma 2 gives two positions with
differing encoded pairs; conditioning on all other key words, two independent
applications of Lemma 1 give probability `≤ 2⁻⁶⁴ · 2⁻⁶⁴`. The column
independence (Lemma 3) translates the NH-level collision to the output. -/
theorem chunk_axu {F : Type} [Field F] [CharP F 2] (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (c c' : Chunk) (hn : c.n = c'.n)
    (hdiff : ∃ b, b < c.n ∧ c.blocks b ≠ c'.blocks b)
    (d : F × F) :
    (Nat.card {K : KeyTable //
      chunkDigest ι c K + chunkDigest ι c' K = d} : ℚ)
      / Nat.card KeyTable ≤ 1 / 2 ^ 128 :=
  ChunkProof.chunk_axu ι hι c c' hn hdiff d

/-- **Theorem 5 (RaijuHash is `(⌈L/8192⌉ + 1) / 2¹²⁸`-AXU).**

For distinct messages of at most `L` bytes and any target `d`, the probability
that `H(m) ⊕ H(m') = d` is at most `(⌈L/8192⌉ + 1) / 2¹²⁸`.

The proof follows SPEC.md Theorem 5:
- **Different lengths, at least one long**: `T` is independent; the output
  difference has a nonzero coefficient on `T`, so it is uniform (`2⁻¹²⁸`).
- **Both short**: the encoding is injective, giving a nonzero coefficient on
  `A` or `B` (`2⁻¹²⁸`).
- **Both long, equal length**: the outer accumulator is a bivariate polynomial
  of degree ≤ `q` in `(R, R2)`. If some chunk pair differs, Schwartz–Zippel
  gives `≤ q / 2¹²⁸`. If all chunk pairs agree, Proposition 4 gives
  `≤ 2⁻¹²⁸`. Total: `(q + 1) / 2¹²⁸`. -/
theorem raijuhash_axu (F : Type) [Field F] [Fintype F] [CharP F 2]
    (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (L : ℕ) (m m' : List Byte) (hm : m ≠ m')
    (hL : m.length ≤ L) (hL' : m'.length ≤ L) (h64 : L < 2 ^ 64) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 :=
  FullProof.raijuhash_axu F hF ι hι L m m' hm hL hL' h64 d

audit_axioms keyTable_card_pos
audit_axioms hashKey_card_pos
audit_axioms nh_axu
audit_axioms chain_distance_two
audit_axioms chunk_axu
audit_axioms raijuhash_axu
audit_axioms fPoly_irreducible
audit_axioms card_GF128
audit_axioms ιGF_kernel
audit_axioms raijuhash_axu_gf128
audit_axioms Reference.reduce256_spec
audit_axioms Reference.gfMul_spec
audit_axioms Reference.chunkPair_spec
audit_axioms Reference.hash_spec
audit_axioms Reference.reference_hash_axu
audit_axioms Reference.Vectors.pattern_0
audit_axioms Reference.Vectors.pattern_1
audit_axioms Reference.Vectors.pattern_15
audit_axioms Reference.Vectors.pattern_16
audit_axioms Reference.Vectors.pattern_31
audit_axioms Reference.Vectors.pattern_32
audit_axioms Reference.Vectors.pattern_127
audit_axioms Reference.Vectors.pattern_128
audit_axioms Reference.Vectors.pattern_129
audit_axioms Reference.Vectors.pattern_1000
audit_axioms Reference.Vectors.pattern_1024

end RaijuHash

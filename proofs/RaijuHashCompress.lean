import Mathlib

/-!
# The chunk compression function of RaijuHash

Models sections 4.1–4.2 of `crates/raijuhash/SPEC.md`. The statement about
it is in `RaijuHashCheck.lean`.

## Model

* A 64-bit word is a polynomial over `𝔽₂` of degree `< 64`, bit `i` being
  the coefficient of `Xⁱ`. XOR is addition.
* `clmul` is carryless multiplication: the polynomial product, of degree
  `< 128`, not reduced modulo anything.
* A 128-bit value is an element of `F = GF(2¹²⁸)`; `ι` maps a polynomial of
  degree `< 128` to the corresponding field element.

### Chain coding (§4.2)

With `w[-1] = w[n] = 0`, encoded position `j` in lane `l` is:

```
tx[j][l] = w[j-1][l]     ⊕ w[j][l]     ⊕ K[j][l]
ty[j][l] = w[j-1][8 + l] ⊕ w[j][8 + l] ⊕ K[j][8 + l]
S[j]     = ⊕_l clmul(tx[j][l], ty[j][l])
```

Position sums are combined with columns `(1, j)` for `j < 64` and `(0, 1)`
for `j = 64`:

```
h0 = ⊕_{j ≤ min(n,63)} S[j]
h1 = ⊕_{j ≤ min(n,63)} j * S[j]  +  (S[64] if n = 64)
```
-/

namespace RaijuHash

open Polynomial

/-- A 64-bit word: a polynomial over `𝔽₂` of degree `< 64`. -/
abbrev Word : Type := degreeLT (ZMod 2) 64

/-- The polynomial ring. -/
abbrev R : Type := (ZMod 2)[X]

/-- Carryless multiplication of two words. -/
noncomputable def clmul (x y : Word) : R := (x : R) * y

/-- Eight lanes per block. -/
abbrev Lane := Fin 8

/-- A block: 16 words = 8 X-words and 8 Y-words. -/
abbrev Block := Lane → Word × Word

/-- The key row for a single encoded position: 8 X-words and 8 Y-words. -/
abbrev KeyRow := Lane → Word × Word


/-- The encoded input at position `j` of lane `l`, given the current and
    previous blocks' lane words and the key row's lane words (§4.2). -/
noncomputable def encode (prev cur key : Word × Word) : Word × Word :=
  (prev.1 + cur.1 + key.1, prev.2 + cur.2 + key.2)

/-- The NH product at one position of one lane. -/
noncomputable def nhLane (tx ty : Word) : R := clmul tx ty

/-- The position sum `S[j]`: XOR over all 8 lanes of the carryless product. -/
noncomputable def positionSum (tx ty : Lane → Word) : R :=
  ∑ l : Lane, nhLane (tx l) (ty l)

/-- A chunk: at most 64 blocks. The number of blocks `n` satisfies `1 ≤ n ≤ 64`. -/
structure Chunk where
  blocks : ℕ → Block
  n : ℕ
  hn_pos : 1 ≤ n
  hn_le : n ≤ 64

/-- The key table: row `j` for position `j`. We need rows `0..=n`, i.e. up to
    65 rows (for a full 64-block chunk, positions 0 through 64). -/
abbrev KeyTable := Fin 65 → KeyRow

instance : Finite (Word : Type) :=
  Finite.of_equiv (Fin 64 → ZMod 2) (degreeLTEquiv (ZMod 2) 64).toEquiv.symm

/-- Bits interpreted as polynomial coefficients, never as a natural-number
    cast into a characteristic-two field. -/
noncomputable def bitPoly (width n : ℕ) : R :=
  ((degreeLTEquiv (ZMod 2) width).symm
    (fun i => if n.testBit i then 1 else 0) : degreeLT (ZMod 2) width)

noncomputable def lenPoly (n : ℕ) : R := bitPoly 64 n

/-- Positions outside the finite key table are unused by chunkDigest. -/
noncomputable def keyAt (K : KeyTable) (j : ℕ) : KeyRow :=
  if h : j < 65 then K ⟨j, h⟩ else 0

/-- The encoded pair at position `j`, lane `l`, for a chunk with `n` blocks.
    `w[-1] = w[n] = 0`, so `prev` at `j = 0` is zero and `cur` at `j = n`
    is zero. -/
noncomputable def encodedPair (c : Chunk) (K : KeyTable) (j : ℕ) (l : Lane) : Word × Word :=
  let prev : Word × Word := if j = 0 then (0, 0) else (c.blocks (j - 1) l)
  let cur : Word × Word := if j = c.n then (0, 0) else (c.blocks j l)
  encode prev cur (keyAt K j l)

/-- `S[j]` for a chunk: the position sum at position `j`. -/
noncomputable def chunkS (c : Chunk) (K : KeyTable) (j : ℕ) : R :=
  ∑ l : Lane, clmul (encodedPair c K j l).1 (encodedPair c K j l).2

/-- The chunk digest `(h0, h1)` per §4.2.
    `h0 = ⊕_{j=0}^{min(n,63)} S[j]`
    `h1 = ⊕_{j=0}^{min(n,63)} j * S[j]  +  (S[64] if n = 64)` -/
noncomputable def chunkDigest {F : Type} [Field F] [CharP F 2]
    (ι : R →+ F) (c : Chunk) (K : KeyTable) : F × F :=
  let bound := min c.n 63
  let h0 := ∑ j ∈ Finset.range (bound + 1), ι (chunkS c K j)
  let h1 := (∑ j ∈ Finset.range (bound + 1), ι (bitPoly 6 j) * ι (chunkS c K j))
    + if c.n = 64 then ι (chunkS c K 64) else 0
  (h0, h1)

end RaijuHash

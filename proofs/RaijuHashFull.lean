import RaijuHash.Encoding

/-!
# The full hash RaijuHash

Defines the complete hash function per `crates/raijuhash/SPEC.md` sections 3–4,
reusing `chunkDigest` from `RaijuHashCompress.lean`. The statement about it is
in `RaijuHashCheck.lean`.

## Structure

The full hash has three cases:
- **Short** (`L < 32`): direct field multiplications with keys `A`, `B`, `S`.
- **Long** (`L ≥ 32`): chunk compression → bivariate polynomial accumulator
  with keys `R`, `R2`, `T`, `S`.
-/

namespace RaijuHash

open Polynomial

/-- A byte, as its eight bits. -/
abbrev Byte : Type := Fin 8 → ZMod 2

/-- Byte `n` of `m`, zero past the end. -/
def byteAt (m : List Byte) (n : ℕ) : Byte := m.getD n 0

/-- Word `n` of `m`, loaded little-endian, zero past the end. -/
noncomputable def wordAt (m : List Byte) (n : ℕ) : Word :=
  (degreeLTEquiv (ZMod 2) 64).symm fun k =>
    byteAt m (8 * n + k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩

/-- The number of 128-byte blocks after zero-padding. -/
def numBlocks (m : List Byte) : ℕ := (m.length + 127) / 128

/-- The number of 8192-byte chunks. -/
def numChunks (m : List Byte) : ℕ := (numBlocks m + 63) / 64

/-- The full key for the hash (excluding `V` which is avalanche-only). -/
structure HashKey (F : Type) where
  /-- The 65 key rows. -/
  table : KeyTable
  /-- Short-message multiplier. -/
  a : F
  /-- Short-message second multiplier. -/
  b : F
  /-- Outer polynomial: first variable. -/
  r : F
  /-- Outer polynomial: second variable. -/
  r2 : F
  /-- Length multiplier. -/
  t : F
  /-- Output offset. -/
  s : F

/-- All sampled keys have a finite, nonempty sample space. -/
def hashKeyEquiv (F : Type) : HashKey F ≃ KeyTable × F × F × F × F × F × F where
  toFun k := (k.table, k.a, k.b, k.r, k.r2, k.t, k.s)
  invFun k := ⟨k.1, k.2.1, k.2.2.1, k.2.2.2.1, k.2.2.2.2.1,
    k.2.2.2.2.2.1, k.2.2.2.2.2.2⟩
  left_inv _ := rfl
  right_inv _ := rfl

instance {F : Type} [Finite F] : Finite (HashKey F) :=
  Finite.of_equiv _ (hashKeyEquiv F).symm

noncomputable instance {F : Type} [Zero F] : Inhabited (HashKey F) :=
  ⟨⟨0, 0, 0, 0, 0, 0, 0⟩⟩

lemma hashKey_card_pos (F : Type) [Finite F] [Zero F] : 0 < Nat.card (HashKey F) :=
  Nat.card_pos

variable {F : Type} [Field F] [CharP F 2]

/-- A 16-byte little-endian polynomial, with zero padding. -/
noncomputable def bytes128 (m : List Byte) : R :=
  ((degreeLTEquiv (ZMod 2) 128).symm
    (fun k => byteAt m (k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩)
    : degreeLT (ZMod 2) 128)

/-- The short-message encoding, including the total length in byte 15. -/
noncomputable def shortX0 (ι : R →+ F) (m : List Byte) : F :=
  ι (bytes128 m + bitPoly 8 m.length * X ^ 120)

noncomputable def hashShort1 (ι : R →+ F) (k : HashKey F) (m : List Byte) : F :=
  shortX0 ι m * k.a + k.s

noncomputable def hashShort2 (ι : R →+ F) (k : HashKey F) (m : List Byte) : F :=
  ι (bytes128 m) * k.a +
    ι (bytes128 (m.drop 16) + bitPoly 8 m.length * X ^ 120) * k.b + k.s

/-- Only valid chunk indices can construct a nonempty chunk. -/
noncomputable def msgChunk (m : List Byte) (i : Fin (numChunks m)) : Chunk where
  blocks j l := (wordAt m (16 * (64 * i.val + j) + l.1),
                 wordAt m (16 * (64 * i.val + j) + 8 + l.1))
  n := min 64 (numBlocks m - 64 * i.val)
  hn_pos := by
    have hi := i.isLt
    simp only [numChunks] at hi
    omega
  hn_le := min_le_left _ _

/-- The bivariate outer polynomial accumulator (§4.3):
    `P_i = (P_{i-1} + h0_i) * R + h1_i * R2`
    starting from `P_0 = 0`. -/
def accumR (r r2 : F) (ps : List (F × F)) : F :=
  ps.foldl (fun acc p => (acc + p.1) * r + p.2 * r2) 0

/-- The full long hash `H(m)`:
    `P_q + L * T + S` where `P_q` is the accumulator over chunk digests. -/
noncomputable def hashLong (ι : R →+ F) (k : HashKey F) (m : List Byte) : F :=
  let q := numChunks m
  let digests : List (F × F) :=
    (List.finRange q).map fun i => chunkDigest ι (msgChunk m i) k.table
  let pq := accumR k.r k.r2 digests
  pq + ι (lenPoly m.length) * k.t + k.s

/-- The complete hash function `H` from §3–4. -/
noncomputable def raijuhash (ι : R →+ F) (k : HashKey F) (m : List Byte) : F :=
  if m.length < 16 then hashShort1 ι k m
  else if m.length < 32 then hashShort2 ι k m
  else hashLong ι k m

end RaijuHash

import Mathlib
import RaijuHashCompress

namespace RaijuHash.ChainProof

open Polynomial

/-- The per-block difference in lane `l`. Returns `(0, 0)` for `b` outside
    `0..n-1` (matching the convention that `w[-1] = w[n] = 0`). -/
noncomputable def blockDiff (c c' : Chunk) (l : Lane) (b : ℕ) : Word × Word :=
  if b < c.n then
    ((c.blocks b l).1 + (c'.blocks b l).1,
     (c.blocks b l).2 + (c'.blocks b l).2)
  else (0, 0)

lemma blockDiff_ne_zero_of_ne (c c' : Chunk) (l : Lane) (b : ℕ) (hb : b < c.n)
    (hne : c.blocks b l ≠ c'.blocks b l) :
    blockDiff c c' l b ≠ (0, 0) := by
  simp only [blockDiff, hb, ite_true]
  intro h
  rw [Prod.ext_iff] at h
  apply hne
  rw [Prod.ext_iff]
  constructor
  · have h1 : ((c.blocks b l).1 : R) + ((c'.blocks b l).1 : R) = 0 := by
      exact congrArg Subtype.val h.1
    have h_two : (2 : R) = 0 := rfl
    apply Subtype.ext
    calc ((c.blocks b l).1 : R)
      _ = ((c.blocks b l).1 : R) + 0 := by ring
      _ = ((c.blocks b l).1 : R) + (((c.blocks b l).1 : R) + ((c'.blocks b l).1 : R)) := by rw [←h1]
      _ = ((c.blocks b l).1 : R) * 2 + ((c'.blocks b l).1 : R) := by ring
      _ = ((c.blocks b l).1 : R) * 0 + ((c'.blocks b l).1 : R) := by rw [h_two]
      _ = ((c'.blocks b l).1 : R) := by ring
  · have h2 : ((c.blocks b l).2 : R) + ((c'.blocks b l).2 : R) = 0 := by
      exact congrArg Subtype.val h.2
    have h_two : (2 : R) = 0 := rfl
    apply Subtype.ext
    calc ((c.blocks b l).2 : R)
      _ = ((c.blocks b l).2 : R) + 0 := by ring
      _ = ((c.blocks b l).2 : R) + (((c.blocks b l).2 : R) + ((c'.blocks b l).2 : R)) := by rw [←h2]
      _ = ((c.blocks b l).2 : R) * 2 + ((c'.blocks b l).2 : R) := by ring
      _ = ((c.blocks b l).2 : R) * 0 + ((c'.blocks b l).2 : R) := by rw [h_two]
      _ = ((c'.blocks b l).2 : R) := by ring
  
end RaijuHash.ChainProof

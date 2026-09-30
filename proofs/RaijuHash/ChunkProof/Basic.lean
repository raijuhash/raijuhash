import RaijuHash.TwoNH

namespace RaijuHash.ChunkProof
open Polynomial
variable {F : Type} [Field F] [CharP F 2]

/-- **Lemma 3.** The column vectors `(1, j)` for `j < 64` and `(0, 1)` are
    pairwise linearly independent: any two of them form an invertible `2 × 2`
    matrix over `F`. -/
lemma columns_independent (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (j₁ j₂ : ℕ) (hlt : j₁ < j₂) (_hj₂ : j₂ ≤ 64) :
    (if j₂ < 64 then ι (bitPoly 6 j₂) + ι (bitPoly 6 j₁) ≠ 0
     else (1 : F) ≠ 0) := by
  split_ifs with h
  · intro heq
    have he := map_bitPoly_injective ι hι (by norm_num)
      (show j₂ < 2 ^ 6 by omega) (show j₁ < 2 ^ 6 by omega)
      (CharTwo.add_eq_zero.mp heq)
    omega
  · exact one_ne_zero

end RaijuHash.ChunkProof

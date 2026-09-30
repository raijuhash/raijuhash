import RaijuHash.OuterPolynomial

namespace RaijuHash.FullProof
open Polynomial
variable {F : Type} [Field F]

/-- The length encoding is injective below the implementation's 64-bit limit. -/
lemma lenPoly_injective {a b : ℕ} (ha : a < 2 ^ 64) (hb : b < 2 ^ 64)
    (h : lenPoly a = lenPoly b) : a = b := bitPoly_injective ha hb h

lemma ι_lenPoly_injective (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    {a b : ℕ} (ha : a < 2 ^ 64) (hb : b < 2 ^ 64)
    (h : ι (lenPoly a) = ι (lenPoly b)) : a = b :=
  map_bitPoly_injective ι hι (by norm_num) ha hb h

lemma numChunks_eq (m : List Byte) : numChunks m = (m.length + 8191) / 8192 := by
  simp only [numChunks, numBlocks]
  omega

lemma numChunks_le {L : ℕ} {m : List Byte} (hL : m.length ≤ L) :
    numChunks m ≤ (L + 8191) / 8192 := by
  rw [numChunks_eq]
  omega

/-- Schwartz–Zippel needs a nonzero *formal polynomial* and its total degree.
    Bounds on nonzero slices of an arbitrary function are insufficient. -/
lemma schwartz_zippel_bivariate [Fintype F] [DecidableEq F] (p : MvPolynomial (Fin 2) F) (hp : p ≠ 0)
    (q : ℕ) (hdeg : p.totalDegree ≤ q) :
    (Finset.univ.filter (fun r : Fin 2 → F => MvPolynomial.eval r p = 0)).card /
      ((Fintype.card F : ℚ≥0) ^ 2) ≤ (q : ℚ≥0) / Fintype.card F := by
  classical
  have h := MvPolynomial.schwartz_zippel_totalDegree hp (Finset.univ : Finset F)
  simp only [Fintype.piFinset_univ, Finset.card_univ] at h
  have hd : (p.totalDegree : ℚ≥0) ≤ (q : ℚ≥0) := by exact_mod_cast hdeg
  exact h.trans (div_le_div_of_nonneg_right hd (by positivity))

end RaijuHash.FullProof

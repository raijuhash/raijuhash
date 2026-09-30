import RaijuHashFull

namespace RaijuHash
noncomputable section

variable {F : Type} [Field F]
abbrev OuterPoly (F : Type) [CommSemiring F] := MvPolynomial (Fin 2) F

/-- The exact Horner recurrence, lifted to formal polynomials. -/
def outerStep (p : OuterPoly F) (h : F × F) : OuterPoly F :=
  (p + MvPolynomial.C h.1) * MvPolynomial.X 0 +
    MvPolynomial.C h.2 * MvPolynomial.X 1

def outerPoly (hs : List (F × F)) : OuterPoly F := hs.foldl outerStep 0

lemma eval_outerStep (r : Fin 2 → F) (p : OuterPoly F) (h : F × F) :
    MvPolynomial.eval r (outerStep p h) =
      (MvPolynomial.eval r p + h.1) * r 0 + h.2 * r 1 := by
  simp [outerStep]

lemma eval_outerFold (r : Fin 2 → F) (hs : List (F × F)) (p : OuterPoly F) :
    MvPolynomial.eval r (hs.foldl outerStep p) =
      hs.foldl (fun a h => (a + h.1) * r 0 + h.2 * r 1) (MvPolynomial.eval r p) := by
  induction hs generalizing p with
  | nil => rfl
  | cons h hs ih => simpa only [List.foldl_cons, eval_outerStep] using ih (outerStep p h)

lemma eval_outerPoly [CharP F 2] (r r2 : F) (hs : List (F × F)) :
    MvPolynomial.eval ![r, r2] (outerPoly hs) = accumR r r2 hs := by
  simp [outerPoly, eval_outerFold, accumR]

lemma degree_outerStep (p : OuterPoly F) (h : F × F) (n : ℕ)
    (hp : p.totalDegree ≤ n) : (outerStep p h).totalDegree ≤ n + 1 := by
  have h₁ := MvPolynomial.totalDegree_add p (MvPolynomial.C h.1)
  have h₂ := MvPolynomial.totalDegree_mul (p + MvPolynomial.C h.1) (MvPolynomial.X (0 : Fin 2))
  have h₃ := MvPolynomial.totalDegree_mul (MvPolynomial.C h.2 : OuterPoly F) (MvPolynomial.X 1)
  have h₄ := MvPolynomial.totalDegree_add
    ((p + MvPolynomial.C h.1) * MvPolynomial.X (0 : Fin 2))
    (MvPolynomial.C h.2 * MvPolynomial.X 1)
  simp only [MvPolynomial.totalDegree_C, MvPolynomial.totalDegree_X] at h₁ h₂ h₃
  unfold outerStep
  omega

lemma degree_outerFold (hs : List (F × F)) (p : OuterPoly F) (n : ℕ)
    (hp : p.totalDegree ≤ n) : (hs.foldl outerStep p).totalDegree ≤ n + hs.length := by
  induction hs generalizing p n with
  | nil => simpa using hp
  | cons h hs ih =>
    have h := ih (outerStep p h) (n + 1) (degree_outerStep p h n hp)
    simpa only [List.foldl_cons, List.length_cons, Nat.add_assoc, Nat.add_comm 1] using h

lemma degree_outerPoly (hs : List (F × F)) : (outerPoly hs).totalDegree ≤ hs.length := by
  simpa [outerPoly] using degree_outerFold hs (0 : OuterPoly F) 0 (by simp)

end
end RaijuHash

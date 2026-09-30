import RaijuHash.Encoding

namespace RaijuHash
open Polynomial

variable {F : Type} [Field F]

/-- Two distinct ordinary columns determine both NH differences uniquely. -/
lemma columnMix_injective (a b : F) (hab : a ≠ b) :
    Function.Injective (fun uv : F × F => (uv.1 + uv.2, a * uv.1 + b * uv.2)) := by
  rintro ⟨u, v⟩ ⟨u', v'⟩ h
  have h₀ : u + v = u' + v' := congrArg Prod.fst h
  have h₁ : a * u + b * v = a * u' + b * v' := congrArg Prod.snd h
  have hv : (b - a) * (v - v') = 0 := by linear_combination h₁ - a * h₀
  have hv' : v = v' := sub_eq_zero.mp ((mul_eq_zero.mp hv).resolve_left (sub_ne_zero.mpr hab.symm))
  exact Prod.ext (by rw [hv'] at h₀; exact add_right_cancel h₀) hv'

/-- The final column `(0, 1)` is independent of every ordinary column. -/
lemma finalColumnMix_injective (a : F) :
    Function.Injective (fun uv : F × F => (uv.1, a * uv.1 + uv.2)) := by
  rintro ⟨u, v⟩ ⟨u', v'⟩ h
  have hu : u = u' := congrArg Prod.fst h
  have hv : a * u + v = a * u' + v' := congrArg Prod.snd h
  exact Prod.ext hu (by rw [hu] at hv; exact add_left_cancel hv)

noncomputable def nhDifference (ι : R →+ F) (x y x' y' : Word) (ab : Word × Word) : F :=
  ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2))

lemma nh_fiber_card (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (x y x' y' : Word) (hne : (x, y) ≠ (x', y')) (d : F) :
    Nat.card {ab : Word × Word // nhDifference ι x y x' y' ab = d} ≤ 2 ^ 64 := by
  have h := NHProof.card_nh_le x y x' y' hne ι hι d
  rw [Nat.card_prod, NHProof.card_Word] at h
  exact Nat.le_of_mul_le_mul_right h (by positivity)

/-- The precise independent two-NH counting step, after conditioning on all
    other keys. The injective map can be either pair of column types. -/
lemma two_nh_count (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (x y x' y' z w z' w' : Word)
    (hxy : (x, y) ≠ (x', y')) (hzw : (z, w) ≠ (z', w'))
    (mix : F × F → F × F) (hmix : Function.Injective mix) (d : F × F) :
    Nat.card {ab : (Word × Word) × (Word × Word) //
      mix (nhDifference ι x y x' y' ab.1, nhDifference ι z w z' w' ab.2) = d} ≤ 2 ^ 128 := by
  let S := {ab : (Word × Word) × (Word × Word) //
    mix (nhDifference ι x y x' y' ab.1, nhDifference ι z w z' w' ab.2) = d}
  by_cases hs : Nonempty S
  · let witness := Classical.choice hs
    let d₁ := nhDifference ι x y x' y' witness.val.1
    let d₂ := nhDifference ι z w z' w' witness.val.2
    let A := {ab : Word × Word // nhDifference ι x y x' y' ab = d₁}
    let B := {ab : Word × Word // nhDifference ι z w z' w' ab = d₂}
    have determines (s : S) :
        (nhDifference ι x y x' y' s.val.1, nhDifference ι z w z' w' s.val.2) = (d₁, d₂) :=
      hmix (s.property.trans witness.property.symm)
    let f : S → A × B := fun s =>
      (⟨s.val.1, congrArg Prod.fst (determines s)⟩,
       ⟨s.val.2, congrArg Prod.snd (determines s)⟩)
    have hf : Function.Injective f := by
      intro a b h
      exact Subtype.ext (Prod.ext
        (congrArg (fun p : A × B => p.1.val) h)
        (congrArg (fun p : A × B => p.2.val) h))
    calc
      Nat.card S ≤ Nat.card (A × B) := Nat.card_le_card_of_injective f hf
      _ = Nat.card A * Nat.card B := Nat.card_prod _ _
      _ ≤ 2 ^ 64 * 2 ^ 64 := Nat.mul_le_mul
        (nh_fiber_card ι hι x y x' y' hxy d₁) (nh_fiber_card ι hι z w z' w' hzw d₂)
      _ = 2 ^ 128 := by rw [← pow_add]
  · have : IsEmpty S := ⟨fun s => hs ⟨s⟩⟩
    exact (Nat.card_of_isEmpty (α := S)).le.trans (Nat.zero_le _)

end RaijuHash

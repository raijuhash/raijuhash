import RaijuHashFull

namespace RaijuHash

/-- A nonzero affine coefficient on one independently sampled field coordinate
    gives the exact counting bound, with finite nonempty denominators. -/
lemma uniform_coordinate_bound {F A K : Type} [Field F] [Finite F]
    [Finite A] [Nonempty A] [Finite K]
    (e : K ≃ A × F) (f : K → F) (base : A → F) (c : F) (hc : c ≠ 0)
    (hf : ∀ k, f k = base (e k).1 + c * (e k).2) (d : F) :
    (Nat.card {k : K // f k = d} : ℚ) / Nat.card K ≤ 1 / Nat.card F := by
  have hcount : Nat.card {k : K // f k = d} ≤ Nat.card A := by
    apply Nat.card_le_card_of_injective (fun k : {k : K // f k = d} => (e k.val).1)
    intro k k' he
    apply Subtype.ext
    apply e.injective
    apply Prod.ext he
    apply mul_left_cancel₀ hc
    have heq := k.property.trans k'.property.symm
    dsimp at he
    rw [hf, hf, he] at heq
    exact add_left_cancel heq
  have hA : (0 : ℚ) < Nat.card A := by exact_mod_cast Nat.card_pos
  have hF : (0 : ℚ) < Nat.card F := by exact_mod_cast Nat.card_pos
  rw [Nat.card_congr e, Nat.card_prod, Nat.cast_mul]
  apply (div_le_div_iff₀ (mul_pos hA hF) hF).mpr
  simp only [one_mul]
  exact mul_le_mul_of_nonneg_right (by exact_mod_cast hcount) hF.le

/-- Isolate the length key while preserving all the other independent keys. -/
def hashKeyTEquiv (F : Type) :
    HashKey F ≃ (KeyTable × F × F × F × F × F) × F where
  toFun k := ((k.table, k.a, k.b, k.r, k.r2, k.s), k.t)
  invFun p := ⟨p.1.1, p.1.2.1, p.1.2.2.1, p.1.2.2.2.1,
    p.1.2.2.2.2.1, p.2, p.1.2.2.2.2.2⟩
  left_inv _ := rfl
  right_inv _ := rfl

variable {F : Type} [Field F] [CharP F 2]

noncomputable def lengthCoefficient (ι : R →+ F) (m : List Byte) : F :=
  if m.length < 32 then 0 else ι (lenPoly m.length)

lemma raijuhash_change_t (ι : R →+ F) (k : HashKey F) (m : List Byte) (t : F) :
    raijuhash ι {k with t := t} m =
      raijuhash ι {k with t := 0} m + lengthCoefficient ι m * t := by
  by_cases h₁ : m.length < 16
  · have h₂ : m.length < 32 := by omega
    simp [raijuhash, h₁, h₂, lengthCoefficient, hashShort1]
  · by_cases h₂ : m.length < 32
    · simp [raijuhash, h₁, h₂, lengthCoefficient, hashShort2]
    · simp [raijuhash, h₁, h₂, lengthCoefficient, hashLong]
      ring

end RaijuHash

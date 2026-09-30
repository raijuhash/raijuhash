import RaijuHashCompress

/-!
# Proof of Lemma 1: carryless NH is `2⁻⁶⁴`-almost-universal

For distinct word pairs `(x, y) ≠ (x', y')` and uniform independent keys `a, b`,
the NH difference `clmul(x ⊕ a, y ⊕ b) ⊕ clmul(x' ⊕ a, y' ⊕ b)` hits any
fixed target with probability `≤ 2⁻⁶⁴`.

## Outline (SPEC.md Lemma 1)

Let `u = x ⊕ a`, `v = y ⊕ b`, `dx = x ⊕ x'`, `dy = y ⊕ y'`. The difference
equals `clmul(u, dy) ⊕ clmul(dx, v) ⊕ clmul(dx, dy)`.

* If `dy ≠ 0`: fix `v`; `u ↦ clmul(u, dy)` is injective on 64-bit `u`
  (the polynomial ring `GF(2)[x]` is an integral domain), and `u` is uniform
  over `2⁶⁴` values, so at most one `u` gives any target.
* If `dy = 0`: then `dx ≠ 0` and the same argument holds for `v`.

In either case, for each choice of the other word (`2⁶⁴` values), at most one
value of the pivoted word works, giving `≤ 2⁶⁴` keys out of `2¹²⁸`.
-/

namespace RaijuHash.NHProof

open Polynomial

/-- Products of distinct words have degree `< 128`. -/
lemma degree_mul_lt_128 {p q : R} (hp : p.degree < 64) (hq : q.degree < 64) :
    (p * q).degree < 128 := by
  rcases eq_or_ne p 0 with rfl | hp0
  · simp only [zero_mul, degree_zero]; exact WithBot.bot_lt_coe _
  rcases eq_or_ne q 0 with rfl | hq0
  · simp only [mul_zero, degree_zero]; exact WithBot.bot_lt_coe _
  rw [Polynomial.degree_eq_natDegree hp0] at hp
  rw [Polynomial.degree_eq_natDegree hq0] at hq
  rw [Polynomial.degree_mul, Polynomial.degree_eq_natDegree hp0,
    Polynomial.degree_eq_natDegree hq0]
  norm_cast at hp hq ⊢
  omega

/-- Word degree bound. -/
lemma degree_lt_64 (x : Word) : (x : R).degree < 64 := Polynomial.mem_degreeLT.mp x.2

/-- The constant term is `xy + x'y'`; the two key coefficients are
    `y + y'` and `x + x'`. -/
lemma nh_diff_affine (x y x' y' a b : Word) :
    clmul (x + a) (y + b) + clmul (x' + a) (y' + b) =
      clmul x y + clmul x' y' + clmul a (y + y') + clmul (x + x') b := by
  unfold clmul
  push_cast
  have htwo : (2 : R) = 0 := CharP.cast_eq_zero R 2
  linear_combination (a : R) * (b : R) * htwo

lemma card_Word : Nat.card Word = 2 ^ 64 := by
  rw [Nat.card_congr (Polynomial.degreeLTEquiv (ZMod 2) 64).toEquiv, Nat.card_fun,
    Nat.card_zmod, Nat.card_eq_fintype_card, Fintype.card_fin]

/-- Injectivity on bounded polynomials follows from the kernel condition. -/
lemma map_inj_bounded {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ z : R, z.degree < 128 → ι z = 0 → z = 0)
    {p q : R} (hp : p.degree < 128) (hq : q.degree < 128)
    (heq : ι p = ι q) : p = q := by
  apply sub_eq_zero.mp
  apply hι (p - q)
  · exact lt_of_le_of_lt (degree_sub_le _ _) (max_lt hp hq)
  · rw [map_sub, heq, sub_self]

/-- Fixing `b` determines `a` whenever the Y words differ. -/
lemma pivot_a {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ z : R, z.degree < 128 → ι z = 0 → z = 0)
    (x y x' y' : Word) (hy : y ≠ y') (b a₁ a₂ : Word)
    (h : ι (clmul (x + a₁) (y + b)) + ι (clmul (x' + a₁) (y' + b)) =
      ι (clmul (x + a₂) (y + b)) + ι (clmul (x' + a₂) (y' + b))) : a₁ = a₂ := by
  simp only [← map_add, nh_diff_affine] at h
  simp only [map_add, add_left_inj, add_right_inj] at h
  have heq := map_inj_bounded ι hι
    (degree_mul_lt_128 (degree_lt_64 a₁) (degree_lt_64 (y + y')))
    (degree_mul_lt_128 (degree_lt_64 a₂) (degree_lt_64 (y + y'))) h
  have hne : (y : R) + (y' : R) ≠ 0 := by
    intro hz
    exact hy (Subtype.ext (CharTwo.add_eq_zero.mp hz))
  exact Subtype.ext (mul_right_cancel₀ hne heq)

lemma pivot_b {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ z : R, z.degree < 128 → ι z = 0 → z = 0)
    (x y x' y' : Word) (hx : x ≠ x') (a b₁ b₂ : Word)
    (h : ι (clmul (x + a) (y + b₁)) + ι (clmul (x' + a) (y' + b₁)) =
      ι (clmul (x + a) (y + b₂)) + ι (clmul (x' + a) (y' + b₂))) : b₁ = b₂ := by
  apply pivot_a ι hι y x y' x' hx a b₁ b₂
  simpa only [clmul, mul_comm] using h

lemma card_nh_le (x y x' y' : Word) (hne : (x, y) ≠ (x', y'))
    {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ z : R, z.degree < 128 → ι z = 0 → z = 0) (d : M) :
    Nat.card {ab : Word × Word //
      ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d}
      * 2 ^ 64 ≤ Nat.card (Word × Word) := by
  let S := {ab : Word × Word //
    ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d}
  have hbound : Nat.card S ≤ Nat.card Word := by
    by_cases hy : y = y'
    · have hx : x ≠ x' := fun h => hne (Prod.ext h hy)
      apply Nat.card_le_card_of_injective (fun z : S => z.val.1)
      rintro ⟨⟨a₁, b₁⟩, h₁⟩ ⟨⟨a₂, b₂⟩, h₂⟩ ha
      dsimp at ha
      subst a₂
      exact Subtype.ext (Prod.ext rfl (pivot_b ι hι x y x' y' hx a₁ b₁ b₂ (h₁.trans h₂.symm)))
    · apply Nat.card_le_card_of_injective (fun z : S => z.val.2)
      rintro ⟨⟨a₁, b₁⟩, h₁⟩ ⟨⟨a₂, b₂⟩, h₂⟩ hb
      dsimp at hb
      subst b₂
      exact Subtype.ext (Prod.ext (pivot_a ι hι x y x' y' hy b₁ a₁ a₂ (h₁.trans h₂.symm)) rfl)
  simpa only [Nat.card_prod, card_Word] using Nat.mul_le_mul_right (2 ^ 64) hbound

/-- Carryless NH is `2⁻⁶⁴`-almost-XOR-universal. -/
theorem nh_axu {M : Type} [AddCommGroup M]
    (ι : R →+ M) (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (x y x' y' : Word) (hne : (x, y) ≠ (x', y')) (d : M) :
    (Nat.card {ab : Word × Word //
      ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d} : ℚ)
      / Nat.card (Word × Word) ≤ 1 / 2 ^ 64 := by
  have hpos : (0 : ℚ) < Nat.card (Word × Word) := by exact_mod_cast Nat.card_pos
  rw [div_le_div_iff₀ hpos (by positivity), one_mul]
  exact_mod_cast card_nh_le x y x' y' hne ι hι d

end RaijuHash.NHProof

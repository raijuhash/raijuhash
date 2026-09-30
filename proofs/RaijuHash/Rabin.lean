import Mathlib

/-!
# A Rabin-style irreducibility test over `𝔽₂`

A monic polynomial `f` of degree 128 over `𝔽₂` is irreducible when
`f ∣ X^(2¹²⁸) - X` and `X^(2⁶⁴) - X` is coprime to `f`. For an irreducible
factor `g` of degree `d`, the field `𝔽₂[X]/(g)` has `2^d` elements; the
first condition makes `d` divide 128, and the second rules out `d ∣ 64`.
-/

namespace RaijuHash
open Polynomial

/-- The Frobenius of `𝔽₂` commutes with evaluation of `𝔽₂`-polynomials. -/
lemma aeval_pow_two_pow {K : Type} [CommRing K] [Algebra (ZMod 2) K] (y : K)
    (p : (ZMod 2)[X]) (k : ℕ) : aeval y p ^ (2 ^ k) = aeval (y ^ (2 ^ k)) p := by
  induction k with
  | zero => simp
  | succ k ih =>
    rw [pow_succ, pow_mul, ih, ← map_pow, ← ZMod.expand_card, expand_aeval, ← pow_mul]

/-- Exponents are kept abstract (`N = 2¹²⁸`, `M = 2⁶⁴`) so that no unifier
    ever sees a power with a literal exponent of that size. -/
theorem irreducible_of_frobenius {f : (ZMod 2)[X]} (hm : f.Monic) (hdeg : f.natDegree = 128)
    (N M : ℕ) (hN : N = 2 ^ 128) (hM : M = 2 ^ 64)
    (h1 : f ∣ X ^ N - X) (h2 : IsCoprime (X ^ M - X) f) : Irreducible f := by
  have hf0 : f ≠ 0 := hm.ne_zero
  have hfu : ¬ IsUnit f := fun h => by
    have := natDegree_eq_zero_of_isUnit h
    omega
  obtain ⟨g, hg, hgf⟩ := WfDvdMonoid.exists_irreducible_factor hfu hf0
  have hg0 : g ≠ 0 := hg.ne_zero
  have hgm : g.Monic := by
    have hl : g.leadingCoeff ≠ 0 := leadingCoeff_ne_zero.mpr hg0
    unfold Monic
    generalize g.leadingCoeff = c at hl
    fin_cases c
    · exact absurd rfl hl
    · rfl
  have := Fact.mk hg
  obtain ⟨d, hd⟩ : ∃ d, g.natDegree = d := ⟨_, rfl⟩
  have hd0 : 0 < d := hd ▸ natDegree_pos_iff_degree_pos.mpr (degree_pos_of_irreducible hg)
  have hdf : d ≤ 128 := hd ▸ hdeg ▸ natDegree_le_of_dvd hgf hf0
  let pb := AdjoinRoot.powerBasis hg0
  have : Module.Finite (ZMod 2) (AdjoinRoot g) := pb.finite
  have : Finite (AdjoinRoot g) := Module.finite_of_finite (ZMod 2)
  let _ : Fintype (AdjoinRoot g) := Fintype.ofFinite _
  have hcard : Fintype.card (AdjoinRoot g) = 2 ^ d := by
    rw [Module.card_eq_pow_finrank (K := ZMod 2), ZMod.card, pb.finrank,
      AdjoinRoot.powerBasis_dim, hd]
  let α := AdjoinRoot.root g
  have hfix (k : ℕ) (h : α ^ 2 ^ k = α) (x : AdjoinRoot g) : x ^ 2 ^ k = x := by
    induction x using AdjoinRoot.induction_on with
    | ih p => rw [← AdjoinRoot.aeval_eq, aeval_pow_two_pow, h]
  have hroot (e : ℕ) : α ^ e = α ↔ g ∣ X ^ e - X := by
    rw [← AdjoinRoot.mk_eq_zero, map_sub, map_pow, AdjoinRoot.mk_X, sub_eq_zero]
  have hαN : α ^ N = α := (hroot N).mpr (hgf.trans h1)
  rw [hN] at hαN
  have hS128 := hfix 128 hαN
  have hSd (x : AdjoinRoot g) : x ^ 2 ^ d = x := by
    rw [← hcard]; exact FiniteField.pow_card x
  have hmul (j : ℕ) (x : AdjoinRoot g) : x ^ 2 ^ (d * j) = x := by
    induction j with
    | zero => simp
    | succ j ih => rw [Nat.mul_succ, pow_add, pow_mul, ih, hSd]
  -- If every element is fixed by `x ↦ x^(2^k)`, then `2^d ≤ 2^k`.
  have hcount (k : ℕ) (hk : 1 ≤ k) (h : ∀ x : AdjoinRoot g, x ^ 2 ^ k = x) : d ≤ k := by
    classical
    have hp : 1 < 2 ^ k := Nat.one_lt_two_pow (by omega)
    have hP := FiniteField.X_pow_card_sub_X_ne_zero (AdjoinRoot g) hp
    have hsub : (Finset.univ : Finset (AdjoinRoot g)) ⊆
        (X ^ 2 ^ k - X : (AdjoinRoot g)[X]).roots.toFinset := by
      intro x _
      rw [Multiset.mem_toFinset, mem_roots hP]
      simp [h x]
    have := (Finset.card_le_card hsub).trans ((Multiset.toFinset_card_le _).trans
      ((card_roots' _).trans (FiniteField.X_pow_card_sub_X_natDegree_eq (AdjoinRoot g) hp).le))
    rw [Finset.card_univ, hcard] at this
    exact (Nat.pow_le_pow_iff_right (by norm_num)).mp this
  have hdvd : d ∣ 128 := by
    have hr (x : AdjoinRoot g) : x ^ 2 ^ (128 % d) = x := by
      have := hS128 x
      rwa [← Nat.mod_add_div 128 d, pow_add, pow_mul, hmul] at this
    by_contra hnd
    have hr0 : 128 % d ≠ 0 := fun h => hnd (Nat.dvd_of_mod_eq_zero h)
    have := hcount (128 % d) (Nat.one_le_iff_ne_zero.mpr hr0) hr
    have := Nat.mod_lt 128 hd0
    omega
  have hd128 : d = 128 := by
    by_contra hne
    have h64 : d ∣ 64 := by
      obtain ⟨i, hi, rfl⟩ := (Nat.dvd_prime_pow Nat.prime_two).mp
        (show d ∣ 2 ^ 7 by simpa using hdvd)
      have hi7 : i ≠ 7 := fun h => hne (by rw [h]; norm_num)
      exact pow_dvd_pow 2 (by omega : i ≤ 6)
    obtain ⟨j, hj⟩ := h64
    have hαM : α ^ M = α := by rw [hM, hj]; exact hmul j α
    have hg64 : g ∣ X ^ M - X := (hroot M).mp hαM
    exact hg.not_isUnit (h2.isUnit_of_dvd' hg64 hgf)
  have : f = g := eq_of_monic_of_dvd_of_natDegree_le hgm hm hgf (by omega)
  rw [this]
  exact hg

end RaijuHash

import RaijuHash.Encoding

/-!
# Polynomials over `𝔽₂` as natural numbers

`toPoly n` has coefficient `i` equal to bit `i` of `n`. XOR is addition,
left shift is multiplication by a power of `X`, and `clmulN` is the
polynomial product. These let the kernel check polynomial identities by
evaluating natural-number bit operations, with no extra axioms.
-/

namespace RaijuHash
open Polynomial

/-- The polynomial whose coefficient `i` is bit `i` of `n`. -/
noncomputable def toPoly (n : ℕ) : R := bitPoly n n

lemma testBit_eq_false_of_le {n i : ℕ} (h : n ≤ i) : n.testBit i = false :=
  Nat.testBit_lt_two_pow (Nat.lt_two_pow_self.trans_le (Nat.pow_le_pow_right two_pos h))

lemma toPoly_coeff (n i : ℕ) : (toPoly n).coeff i = if n.testBit i then 1 else 0 := by
  by_cases hi : i < n
  · exact bitPoly_coeff n n ⟨i, hi⟩
  · rw [testBit_eq_false_of_le (not_lt.mp hi)]
    exact coeff_eq_zero_of_degree_lt
      ((bitPoly_degree n n).trans_le (by exact_mod_cast not_lt.mp hi))

lemma toPoly_ext {p : R} {n : ℕ} (h : ∀ i, p.coeff i = if n.testBit i then 1 else 0) :
    p = toPoly n := by
  ext i
  rw [h, toPoly_coeff]

lemma bitPoly_eq_toPoly {w n : ℕ} (h : n < 2 ^ w) : bitPoly w n = toPoly n := by
  apply toPoly_ext
  intro i
  by_cases hi : i < w
  · exact bitPoly_coeff w n ⟨i, hi⟩
  · rw [Nat.testBit_lt_two_pow (h.trans_le (Nat.pow_le_pow_right two_pos (not_lt.mp hi)))]
    exact coeff_eq_zero_of_degree_lt
      ((bitPoly_degree w n).trans_le (by exact_mod_cast not_lt.mp hi))

lemma toPoly_degree {w n : ℕ} (h : n < 2 ^ w) : (toPoly n).degree < w := by
  rw [← bitPoly_eq_toPoly h]
  exact bitPoly_degree w n

lemma toPoly_injective {a b : ℕ} (h : toPoly a = toPoly b) : a = b := by
  apply Nat.eq_of_testBit_eq
  intro i
  have hc := congrArg (fun p : R => p.coeff i) h
  simp only [toPoly_coeff] at hc
  cases h₁ : a.testBit i <;> cases h₂ : b.testBit i <;> simp_all

lemma bit_add (x y : Bool) :
    ((if x then 1 else 0 : ZMod 2) + if y then 1 else 0) = if xor x y then 1 else 0 := by
  cases x <;> cases y <;> decide

@[simp] lemma toPoly_zero : toPoly 0 = 0 := by
  symm; apply toPoly_ext; intro i; simp

lemma toPoly_xor (a b : ℕ) : toPoly (a ^^^ b) = toPoly a + toPoly b := by
  symm; apply toPoly_ext; intro i
  rw [coeff_add, toPoly_coeff, toPoly_coeff, Nat.testBit_xor, bit_add]

lemma toPoly_shiftLeft (a k : ℕ) : toPoly (a <<< k) = X ^ k * toPoly a := by
  symm; apply toPoly_ext; intro i
  rw [coeff_X_pow_mul', Nat.testBit_shiftLeft, toPoly_coeff]
  by_cases h : k ≤ i <;> simp [h]

lemma toPoly_two_pow (k : ℕ) : toPoly (2 ^ k) = X ^ k := by
  rw [← Nat.one_shiftLeft, toPoly_shiftLeft]
  have : toPoly 1 = 1 := by
    symm; apply toPoly_ext; intro i
    rw [coeff_one]
    rcases i with _ | i <;> simp [Nat.testBit_succ]
  rw [this, mul_one]

@[simp] lemma toPoly_one : toPoly 1 = 1 := by
  simpa using toPoly_two_pow 0

@[simp] lemma toPoly_two : toPoly 2 = X := by
  simpa using toPoly_two_pow 1

lemma toPoly_mod_two_pow_succ (b k : ℕ) :
    toPoly (b % 2 ^ (k + 1)) = toPoly (b % 2 ^ k) + if b.testBit k then X ^ k else 0 := by
  symm; apply toPoly_ext; intro i
  rw [coeff_add, toPoly_coeff, Nat.testBit_mod_two_pow, Nat.testBit_mod_two_pow]
  split_ifs with h <;> simp only [coeff_X_pow, coeff_zero] <;>
    rcases Nat.lt_or_ge i k with hi | hi <;> rcases eq_or_ne i k with hk | hk <;>
    simp_all <;> omega

/-- Carryless multiplication by the low `k` bits of `b`. -/
def clmulAux (a b : ℕ) : ℕ → ℕ
  | 0 => 0
  | k + 1 => clmulAux a b k ^^^ (if b.testBit k then a <<< k else 0)

lemma toPoly_clmulAux (a b k : ℕ) :
    toPoly (clmulAux a b k) = toPoly a * toPoly (b % 2 ^ k) := by
  induction k with
  | zero => simp [clmulAux, Nat.mod_one]
  | succ k ih =>
    rw [clmulAux, toPoly_xor, ih, toPoly_mod_two_pow_succ, mul_add]
    split_ifs <;> simp [toPoly_shiftLeft, mul_comm]

/-- Carryless multiplication of natural numbers. -/
def clmulN (a b : ℕ) : ℕ := clmulAux a b (Nat.log2 b + 1)

lemma toPoly_clmulN (a b : ℕ) : toPoly (clmulN a b) = toPoly a * toPoly b := by
  rw [clmulN, toPoly_clmulAux, Nat.mod_eq_of_lt]
  rw [Nat.log2_eq_log_two]
  exact Nat.lt_pow_succ_log_self (by norm_num) b

end RaijuHash


import RaijuHash.NHProof.Main

namespace RaijuHash
open Polynomial

lemma bitPoly_zero (width : ℕ) : bitPoly width 0 = 0 := by
  unfold bitPoly
  have h : (fun i : Fin width => if (0 : ℕ).testBit i then (1 : ZMod 2) else 0) = 0 := by
    ext i
    simp
  rw [h]
  simp

lemma bitPoly_degree (width n : ℕ) : (bitPoly width n).degree < width :=
  mem_degreeLT.mp ((degreeLTEquiv (ZMod 2) width).symm
    (fun i => if n.testBit i then 1 else 0)).property

lemma bitPoly_coeff (width n : ℕ) (i : Fin width) :
    (bitPoly width n).coeff i = if n.testBit i then 1 else 0 := by
  exact congrFun ((degreeLTEquiv (ZMod 2) width).apply_symm_apply
    (fun i => if n.testBit i then 1 else 0)) i

lemma bitPoly_injective {width a b : ℕ} (ha : a < 2 ^ width) (hb : b < 2 ^ width)
    (h : bitPoly width a = bitPoly width b) : a = b := by
  apply Nat.eq_of_testBit_eq
  intro i
  by_cases hi : i < width
  · have hc := congrArg (fun p : R => p.coeff i) h
    rw [bitPoly_coeff width a ⟨i, hi⟩, bitPoly_coeff width b ⟨i, hi⟩] at hc
    cases h₁ : a.testBit i <;> cases h₂ : b.testBit i <;> simp_all
  · have hp : 2 ^ width ≤ 2 ^ i := Nat.pow_le_pow_right (by decide) (by omega)
    rw [Nat.testBit_lt_two_pow (ha.trans_le hp), Nat.testBit_lt_two_pow (hb.trans_le hp)]

lemma map_bitPoly_injective {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    {width a b : ℕ} (hw : width ≤ 128) (ha : a < 2 ^ width) (hb : b < 2 ^ width)
    (h : ι (bitPoly width a) = ι (bitPoly width b)) : a = b := by
  apply bitPoly_injective ha hb
  apply NHProof.map_inj_bounded ι hι _ _ h
  · exact (bitPoly_degree width a).trans_le (by exact_mod_cast hw)
  · exact (bitPoly_degree width b).trans_le (by exact_mod_cast hw)

lemma card_KeyTable : Nat.card KeyTable = 2 ^ (65 * 16 * 64) := by
  simp only [KeyTable, KeyRow, Lane, Nat.card_fun, Nat.card_prod,
    NHProof.card_Word, Nat.card_eq_fintype_card, Fintype.card_fin]
  simp only [← pow_add, ← pow_mul]

lemma keyTable_card_pos : 0 < Nat.card KeyTable := Nat.card_pos

end RaijuHash

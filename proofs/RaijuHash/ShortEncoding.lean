import RaijuHashFull

set_option maxRecDepth 4096

namespace RaijuHash
open Polynomial

lemma bytes128_coeff (m : List Byte) (i : Fin 128) :
    (bytes128 m).coeff i = byteAt m (i / 8) ⟨i % 8, Nat.mod_lt _ (by norm_num)⟩ := by
  exact congrFun ((degreeLTEquiv (ZMod 2) 128).apply_symm_apply
    (fun k => byteAt m (k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩)) i

lemma bytes128_degree (m : List Byte) : (bytes128 m).degree < 128 :=
  mem_degreeLT.mp ((degreeLTEquiv (ZMod 2) 128).symm
    (fun k => byteAt m (k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩)).property

lemma bytes128_injective {m m' : List Byte} (hlen : m.length = m'.length)
    (hsmall : m.length ≤ 16) (h : bytes128 m = bytes128 m') : m = m' := by
  apply List.ext_getElem hlen
  intro i hi hi'
  funext b
  have hb := b.isLt
  have hc := congrArg (fun p : R => p.coeff (8 * i + b.val)) h
  rw [bytes128_coeff m ⟨8 * i + b.val, by omega⟩,
    bytes128_coeff m' ⟨8 * i + b.val, by omega⟩] at hc
  have hd : (8 * i + b.val) / 8 = i := by omega
  have hr : (8 * i + b.val) % 8 = b.val := by omega
  simpa only [hd, hr, byteAt, List.getD_eq_getElem m 0 hi, List.getD_eq_getElem m' 0 hi'] using hc

lemma bytes128_byte15 (m : List Byte) (hlen : m.length ≤ 15) (b : Fin 8) :
    (bytes128 m).coeff (b.val + 120) = 0 := by
  have hb := b.isLt
  rw [bytes128_coeff m ⟨b.val + 120, by omega⟩]
  have hd : (b.val + 120) / 8 = 15 := by omega
  simp only [hd, byteAt, List.getD_eq_default m 0 hlen, Pi.zero_apply]

/-- A length marker in byte 15 survives zero padding; this was absent from
    the original model. -/
lemma taggedPoly_coeff (m : List Byte) (hlen : m.length ≤ 15) (n : ℕ) (b : Fin 8) :
    (bytes128 m + bitPoly 8 n * X ^ 120).coeff (b.val + 120) =
      if n.testBit b then 1 else 0 := by
  rw [coeff_add, bytes128_byte15 m hlen, coeff_mul_X_pow, zero_add, bitPoly_coeff]

lemma taggedPoly_length_injective (m m' : List Byte)
    (hm : m.length ≤ 15) (hm' : m'.length ≤ 15)
    (n n' : ℕ) (hn : n < 256) (hn' : n' < 256)
    (h : bytes128 m + bitPoly 8 n * X ^ 120 =
      bytes128 m' + bitPoly 8 n' * X ^ 120) : n = n' := by
  apply Nat.eq_of_testBit_eq
  intro i
  by_cases hi : i < 8
  · have hc := congrArg (fun p : R => p.coeff (i + 120)) h
    rw [taggedPoly_coeff m hm n ⟨i, hi⟩, taggedPoly_coeff m' hm' n' ⟨i, hi⟩] at hc
    cases h₁ : n.testBit i <;> cases h₂ : n'.testBit i <;> simp_all
  · have hp : 256 ≤ 2 ^ i := (by decide : 256 = 2 ^ 8) ▸
      Nat.pow_le_pow_right (by decide) (by omega : 8 ≤ i)
    rw [Nat.testBit_lt_two_pow (hn.trans_le hp), Nat.testBit_lt_two_pow (hn'.trans_le hp)]

end RaijuHash

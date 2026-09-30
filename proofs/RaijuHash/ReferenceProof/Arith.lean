import RaijuHash.Reference
import RaijuHash.GF128

/-!
# `reference.rs` arithmetic is polynomial and field arithmetic

`toPoly` reads a Rust integer as a polynomial over `𝔽₂`. `clmul64` is the
polynomial product (`toPoly_clmul64`), and `gf_mul` is multiplication in
`GF128`, returning a value below `2¹²⁸` (`gfMul_spec`). The key step is
`reduce256_spec`: the two folds of `reduce256` replace `X¹²⁸` by
`X⁷ + X² + X + 1`, and the second fold never overflows.
-/

namespace RaijuHash.Reference
open Polynomial RaijuHash

/-! ### Bits -/

lemma testBit_fromLE (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) (k : ℕ) :
    (fromLE bs).testBit k = (bs.getD (k / 8) 0).testBit (k % 8) := by
  induction bs generalizing k with
  | nil => simp [fromLE]
  | cons b bs ih =>
    have hb0 : b < 2 ^ 8 := hb b (by simp)
    have hrest : ∀ b ∈ bs, b < 256 := fun x hx => hb x (by simp [hx])
    change (b + 256 * fromLE bs).testBit k = _
    rw [add_comm, show (256 : ℕ) = 2 ^ 8 by norm_num, Nat.testBit_two_pow_mul_add _ hb0]
    by_cases hk : k < 8
    · simp only [hk, ↓reduceIte, show k / 8 = 0 by omega, show k % 8 = k by omega]
      rfl
    · simp only [hk, ↓reduceIte]
      rw [ih hrest]
      have h1 : k / 8 = (k - 8) / 8 + 1 := by omega
      have h2 : k % 8 = (k - 8) % 8 := by omega
      rw [h1, h2]
      rfl

lemma fromLE_lt (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) : fromLE bs < 2 ^ (8 * bs.length) := by
  induction bs with
  | nil => simp [fromLE]
  | cons b bs ih =>
    have hb0 : b < 256 := hb b (by simp)
    have := ih fun x hx => hb x (by simp [hx])
    change b + 256 * fromLE bs < 2 ^ (8 * (bs.length + 1))
    rw [Nat.mul_succ, pow_add]
    norm_num
    omega

lemma toPoly_split (n k : ℕ) : toPoly n = toPoly (n % 2 ^ k) + X ^ k * toPoly (n >>> k) := by
  apply (toPoly_ext _).symm
  intro i
  rw [coeff_add, coeff_X_pow_mul', toPoly_coeff, toPoly_coeff, Nat.testBit_mod_two_pow]
  by_cases hi : i < k
  · simp [hi, show ¬ k ≤ i by omega]
  · simp only [hi, decide_false, Bool.false_and, Bool.false_eq_true, ite_false, zero_add,
      show k ≤ i by omega, ite_true, Nat.testBit_shiftRight]
    rw [show k + (i - k) = i by omega]

lemma shiftLeft_shiftRight_128 (h k : ℕ) (hk : k ≤ 128) :
    (h <<< k) >>> 128 = h >>> (128 - k) := by
  rw [Nat.shiftLeft_eq, Nat.shiftRight_eq_div_pow, Nat.shiftRight_eq_div_pow,
    show (128 : ℕ) = (128 - k) + k by omega, pow_add, Nat.mul_div_mul_right _ _ (by positivity)]
  congr 2
  omega

/-- A truncated `u128` shift plus the discarded bits is the full product. -/
lemma toPoly_shl128 (h k : ℕ) (hk : k ≤ 128) :
    toPoly (shl128 h k) + X ^ 128 * toPoly (h >>> (128 - k)) = X ^ k * toPoly h := by
  rw [← toPoly_shiftLeft h k, toPoly_split (h <<< k) 128, shiftLeft_shiftRight_128 h k hk,
    shl128]

lemma shl128_lt (h k : ℕ) : shl128 h k < 2 ^ 128 := Nat.mod_lt _ (by positivity)

lemma shl128_eq {h k : ℕ} (hh : h < 2 ^ 64) (hk : k < 64) : shl128 h k = h <<< k := by
  apply Nat.mod_eq_of_lt
  rw [Nat.shiftLeft_eq]
  calc h * 2 ^ k < 2 ^ 64 * 2 ^ 64 :=
        Nat.mul_lt_mul_of_lt_of_le hh (Nat.pow_le_pow_right (by norm_num) hk.le) (by positivity)
    _ = 2 ^ 128 := by norm_num

/-! ### `clmul64` -/

lemma clmul64_step (b i : ℕ) : ((b >>> i) &&& 1 = 1) ↔ b.testBit i = true := by
  rw [Nat.and_one_is_mod, Nat.testBit_eq_decide_div_mod_eq, Nat.shiftRight_eq_div_pow]
  simp

lemma clmul64_fold (a b : ℕ) (ha : a < 2 ^ 64) (n : ℕ) (hn : n ≤ 64) :
    (List.range n).foldl (fun r i => if (b >>> i) &&& 1 = 1 then r ^^^ shl128 a i else r) 0 =
      clmulAux a b n := by
  induction n with
  | zero => rfl
  | succ n ih =>
    rw [List.range_succ, List.foldl_append, ih (by omega), clmulAux]
    simp only [List.foldl_cons, List.foldl_nil]
    by_cases h : b.testBit n
    · simp only [(clmul64_step b n).mpr h, h, ↓reduceIte, shl128_eq ha (by omega : n < 64)]
    · have h' : ¬ ((b >>> n) &&& 1 = 1) := fun h' => h ((clmul64_step b n).mp h')
      simp only [h', h, ↓reduceIte, Nat.xor_zero, Bool.false_eq_true]

lemma toPoly_clmul64 {a b : ℕ} (ha : a < 2 ^ 64) (hb : b < 2 ^ 64) :
    toPoly (clmul64 a b) = toPoly a * toPoly b := by
  rw [clmul64, clmul64_fold a b ha 64 le_rfl, toPoly_clmulAux, Nat.mod_eq_of_lt hb]

lemma clmul64_lt (a b : ℕ) : clmul64 a b < 2 ^ 128 := by
  unfold clmul64
  suffices ∀ (l : List ℕ) (r : ℕ), r < 2 ^ 128 →
      l.foldl (fun r i => if (b >>> i) &&& 1 = 1 then r ^^^ shl128 a i else r) r < 2 ^ 128 from
    this _ 0 (by positivity)
  intro l
  induction l with
  | nil => exact fun r hr => hr
  | cons i l ih =>
    intro r hr
    apply ih
    dsimp only
    split_ifs
    · exact Nat.xor_lt_two_pow hr (shl128_lt a i)
    · exact hr

/-! ### `reduce256` and `gf_mul` -/

/-- `X⁷ + X² + X + 1`, which is `X¹²⁸` modulo `fPoly`. -/
def gNat : ℕ := 0x87

lemma fNat_eq : fNat = 2 ^ 128 ^^^ gNat := by
  unfold fNat gNat
  decide +kernel

lemma gNat_eq : gNat = 2 ^ 7 ^^^ 2 ^ 2 ^^^ 2 ^ 1 ^^^ 2 ^ 0 := by
  unfold gNat
  decide +kernel

lemma toPoly_gNat : toPoly gNat = X ^ 7 + X ^ 2 + X + 1 := by
  rw [gNat_eq, toPoly_xor, toPoly_xor, toPoly_xor, toPoly_two_pow, toPoly_two_pow,
    toPoly_two_pow, toPoly_two_pow, pow_one, pow_zero]

lemma mk_X_pow_128 : AdjoinRoot.mk fPoly (X ^ 128) = AdjoinRoot.mk fPoly (toPoly gNat) := by
  apply AdjoinRoot.mk_eq_mk.mpr
  refine ⟨1, ?_⟩
  rw [← toPoly_fNat, fNat_eq, toPoly_xor, toPoly_two_pow, mul_one, CharTwo.sub_eq_add]

lemma shiftRight_lt {h k w : ℕ} (hh : h < 2 ^ (w + k)) : h >>> k < 2 ^ w := by
  rw [Nat.shiftRight_eq_div_pow, Nat.div_lt_iff_lt_mul (by positivity), ← pow_add]
  exact hh

lemma reduceFold_spec (h : ℕ) (hh : h < 2 ^ 128) :
    (reduceFold h).1 < 2 ^ 128 ∧ (reduceFold h).2 < 2 ^ 7 ∧
      toPoly (reduceFold h).1 + X ^ 128 * toPoly (reduceFold h).2 = toPoly h * toPoly gNat := by
  refine ⟨?_, ?_, ?_⟩
  · exact Nat.xor_lt_two_pow (Nat.xor_lt_two_pow (Nat.xor_lt_two_pow hh (shl128_lt _ _))
      (shl128_lt _ _)) (shl128_lt _ _)
  · refine Nat.xor_lt_two_pow (Nat.xor_lt_two_pow ?_ ?_) (shiftRight_lt hh)
    · exact (shiftRight_lt (w := 1) hh).trans_le (by norm_num)
    · exact (shiftRight_lt (w := 2) hh).trans_le (by norm_num)
  · have e1 := toPoly_shl128 h 1 (by norm_num)
    have e2 := toPoly_shl128 h 2 (by norm_num)
    have e7 := toPoly_shl128 h 7 (by norm_num)
    simp only [reduceFold, toPoly_xor, toPoly_gNat]
    norm_num at e1 e2 e7
    linear_combination e1 + e2 + e7

lemma reduceFold_small (h : ℕ) (hh : h < 2 ^ 7) : (reduceFold h).2 = 0 := by
  have h0 : ∀ k, 7 ≤ k → h >>> k = 0 := fun k hk => by
    rw [Nat.shiftRight_eq_div_pow]
    exact Nat.div_eq_of_lt (hh.trans_le (Nat.pow_le_pow_right (by norm_num) hk))
  simp only [reduceFold, h0 127 (by norm_num), h0 126 (by norm_num), h0 121 (by norm_num),
    Nat.xor_zero]

lemma reduce256_spec (low high : ℕ) (hl : low < 2 ^ 128) (hh : high < 2 ^ 128) :
    reduce256 low high < 2 ^ 128 ∧
      AdjoinRoot.mk fPoly (toPoly (reduce256 low high)) =
        AdjoinRoot.mk fPoly (toPoly low + X ^ 128 * toPoly high) := by
  obtain ⟨ht, hov, e1⟩ := reduceFold_spec high hh
  obtain ⟨ht2, -, e2⟩ := reduceFold_spec (reduceFold high).2
    (hov.trans (Nat.pow_lt_pow_right (by norm_num) (by norm_num)))
  have hz := reduceFold_small _ hov
  simp only [hz, toPoly_zero, mul_zero, add_zero] at e2
  have hr : reduce256 low high = low ^^^ (reduceFold high).1 ^^^ (reduceFold (reduceFold high).2).1 :=
    rfl
  rw [hr]
  refine ⟨Nat.xor_lt_two_pow (Nat.xor_lt_two_pow hl ht) ht2, ?_⟩
  have E1 := congrArg (AdjoinRoot.mk fPoly) e1
  have E2 := congrArg (AdjoinRoot.mk fPoly) e2
  simp only [map_add, map_mul] at E1 E2
  simp only [toPoly_xor, map_add, map_mul]
  rw [mk_X_pow_128] at E1 ⊢
  linear_combination E1 + E2

lemma gfMul_eq (a b : ℕ) : gfMul a b =
    reduce256 (clmul64 (a % 2 ^ 64) (b % 2 ^ 64) ^^^
        shl128 (clmul64 (a % 2 ^ 64) ((b >>> 64) % 2 ^ 64) ^^^
          clmul64 ((a >>> 64) % 2 ^ 64) (b % 2 ^ 64)) 64)
      (clmul64 ((a >>> 64) % 2 ^ 64) ((b >>> 64) % 2 ^ 64) ^^^
        ((clmul64 (a % 2 ^ 64) ((b >>> 64) % 2 ^ 64) ^^^
          clmul64 ((a >>> 64) % 2 ^ 64) (b % 2 ^ 64)) >>> 64)) := rfl

lemma toPoly_halves {a : ℕ} (ha : a < 2 ^ 128) :
    toPoly a = toPoly (a % 2 ^ 64) + X ^ 64 * toPoly ((a >>> 64) % 2 ^ 64) := by
  rw [Nat.mod_eq_of_lt (shiftRight_lt (w := 64) (by simpa using ha))]
  exact toPoly_split a 64

/-- `gf_mul` is multiplication in `GF128`, and stays below `2¹²⁸`. -/
theorem gfMul_spec {a b : ℕ} (ha : a < 2 ^ 128) (hb : b < 2 ^ 128) :
    gfMul a b < 2 ^ 128 ∧
      AdjoinRoot.mk fPoly (toPoly (gfMul a b)) =
        AdjoinRoot.mk fPoly (toPoly a) * AdjoinRoot.mk fPoly (toPoly b) := by
  have h64 : ∀ x, x % 2 ^ 64 < 2 ^ 64 := fun x => Nat.mod_lt _ (by positivity)
  have hmid := Nat.xor_lt_two_pow (clmul64_lt (a % 2 ^ 64) ((b >>> 64) % 2 ^ 64))
    (clmul64_lt ((a >>> 64) % 2 ^ 64) (b % 2 ^ 64))
  obtain ⟨hlt, hmk⟩ := reduce256_spec _ _
    (Nat.xor_lt_two_pow (clmul64_lt _ _) (shl128_lt _ _))
    (Nat.xor_lt_two_pow (clmul64_lt _ _) ((Nat.shiftRight_le _ _).trans_lt hmid))
  rw [gfMul_eq]
  refine ⟨hlt, ?_⟩
  rw [hmk, ← map_mul, toPoly_halves ha, toPoly_halves hb]
  have e := toPoly_shl128 (clmul64 (a % 2 ^ 64) ((b >>> 64) % 2 ^ 64) ^^^
    clmul64 ((a >>> 64) % 2 ^ 64) (b % 2 ^ 64)) 64 (by norm_num)
  rw [show 128 - 64 = 64 from rfl] at e
  apply congrArg
  simp only [toPoly_xor, toPoly_clmul64 (h64 _) (h64 _)] at e ⊢
  linear_combination e

end RaijuHash.Reference

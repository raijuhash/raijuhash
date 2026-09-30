import RaijuHash.ShortEncoding
import RaijuHash.UniformCoordinate

set_option maxRecDepth 4096

namespace RaijuHash.FullProof
open Polynomial

noncomputable def tagged (m : List Byte) (n : ℕ) : R :=
  bytes128 m + bitPoly 8 n * X ^ 120

lemma tagged_degree (m : List Byte) (n : ℕ) : (tagged m n).degree < 128 := by
  apply lt_of_le_of_lt (degree_add_le _ _) (max_lt (bytes128_degree m) ?_)
  by_cases h : bitPoly 8 n = 0
  · simp only [h, zero_mul, degree_zero]
    exact WithBot.bot_lt_coe _
  · have hd := bitPoly_degree 8 n
    rw [degree_eq_natDegree h] at hd
    rw [degree_mul, degree_X_pow, degree_eq_natDegree h]
    norm_cast at hd ⊢
    omega

lemma bytes128_nil : bytes128 [] = 0 := by
  unfold bytes128
  have h : (fun k : Fin 128 => byteAt [] (k / 8)
      ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩) = 0 := by ext; simp [byteAt]
  rw [h]
  simp

lemma tagged_nil_zero : tagged [] 0 = 0 := by simp [tagged, bytes128_nil, bitPoly_zero]

lemma tagged_injective {m m' : List Byte} (hm : m.length ≤ 15) (hm' : m'.length ≤ 15)
    (h : tagged m m.length = tagged m' m'.length) : m = m' := by
  have hl := taggedPoly_length_injective m m' hm hm' m.length m'.length
    (by omega) (by omega) h
  apply bytes128_injective hl (by omega)
  unfold tagged at h
  rw [hl] at h
  exact add_right_cancel h

lemma bytes128_take (m : List Byte) : bytes128 (m.take 16) = bytes128 m := by
  unfold bytes128
  congr 2
  funext k
  simp only [byteAt, List.getD_eq_getElem?_getD,
    List.getElem?_take_of_lt (show k.val / 8 < 16 by have := k.isLt; omega)]

noncomputable def shortPoly (m : List Byte) : R × R :=
  if m.length < 16 then (tagged m m.length, 0)
  else (bytes128 m, tagged (m.drop 16) m.length)

lemma shortPoly_bounded (m : List Byte) :
    (shortPoly m).1.degree < 128 ∧ (shortPoly m).2.degree < 128 := by
  unfold shortPoly
  split_ifs
  · exact ⟨tagged_degree _ _, WithBot.bot_lt_coe _⟩
  · exact ⟨bytes128_degree _, tagged_degree _ _⟩

lemma tagged_nonzero (m : List Byte) (hm : m.length ≤ 15)
    (n : ℕ) (hn : 0 < n) (hn' : n < 256) : tagged m n ≠ 0 := by
  intro h
  have hz : tagged m n = tagged [] 0 := h.trans tagged_nil_zero.symm
  have := taggedPoly_length_injective m [] hm (by simp) n 0 hn' (by norm_num) hz
  omega

lemma shortPoly_injective {m m' : List Byte} (hm : m.length < 32) (hm' : m'.length < 32)
    (h : shortPoly m = shortPoly m') : m = m' := by
  by_cases h₁ : m.length < 16 <;> by_cases h₂ : m'.length < 16
  · apply tagged_injective (by omega) (by omega)
    simpa only [shortPoly, h₁, h₂, ite_true] using congrArg Prod.fst h
  · have hz := congrArg Prod.snd h
    simp only [shortPoly, h₁, h₂, ite_true, ite_false] at hz
    exact False.elim (tagged_nonzero (m'.drop 16) (by simp; omega)
      m'.length (by omega) (by omega) hz.symm)
  · have hz := congrArg Prod.snd h
    simp only [shortPoly, h₁, h₂, ite_true, ite_false] at hz
    exact False.elim (tagged_nonzero (m.drop 16) (by simp; omega)
      m.length (by omega) (by omega) hz)
  · have hx := congrArg Prod.fst h
    have hy := congrArg Prod.snd h
    simp only [shortPoly, h₁, h₂, ite_false] at hx hy
    have hl := taggedPoly_length_injective (m.drop 16) (m'.drop 16)
      (by simp; omega) (by simp; omega) m.length m'.length (by omega) (by omega) hy
    have hd : m.drop 16 = m'.drop 16 := by
      apply bytes128_injective (by simp [hl]) (by simp; omega)
      unfold tagged at hy
      rw [hl] at hy
      exact add_right_cancel hy
    have ht : m.take 16 = m'.take 16 := by
      apply bytes128_injective (by simp [hl]) (by simp)
      simpa only [bytes128_take] using hx
    calc m = m.take 16 ++ m.drop 16 := (List.take_append_drop 16 m).symm
      _ = m'.take 16 ++ m'.drop 16 := by rw [ht, hd]
      _ = m' := List.take_append_drop 16 m'

variable {F : Type} [Field F]

noncomputable def shortField (ι : R →+ F) (m : List Byte) : F × F :=
  (ι (shortPoly m).1, ι (shortPoly m).2)

lemma shortField_injective (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    {m m' : List Byte} (hm : m.length < 32) (hm' : m'.length < 32)
    (h : shortField ι m = shortField ι m') : m = m' := by
  apply shortPoly_injective hm hm'
  apply Prod.ext
  · exact NHProof.map_inj_bounded ι hι (shortPoly_bounded m).1
      (shortPoly_bounded m').1 (congrArg Prod.fst h)
  · exact NHProof.map_inj_bounded ι hι (shortPoly_bounded m).2
      (shortPoly_bounded m').2 (congrArg Prod.snd h)

variable [CharP F 2]

lemma raijuhash_short (ι : R →+ F) (m : List Byte) (hm : m.length < 32) (k : HashKey F) :
    raijuhash ι k m = (shortField ι m).1 * k.a + (shortField ι m).2 * k.b + k.s := by
  by_cases h : m.length < 16 <;>
    simp [raijuhash, h, hm, hashShort1, hashShort2, shortX0, shortField, shortPoly, tagged]

/-- Put either short-message multiplier last, leaving all other keys in the
    independent first coordinate. -/
def hashKeyShortEquiv (F : Type) (useA : Bool) :
    HashKey F ≃ (KeyTable × F × F × F × F × F) × F :=
  if useA then
    { toFun := fun k => ((k.table, k.b, k.r, k.r2, k.t, k.s), k.a)
      invFun := fun p => ⟨p.1.1, p.2, p.1.2.1, p.1.2.2.1,
        p.1.2.2.2.1, p.1.2.2.2.2.1, p.1.2.2.2.2.2⟩
      left_inv := fun _ => rfl
      right_inv := fun _ => rfl }
  else
    { toFun := fun k => ((k.table, k.a, k.r, k.r2, k.t, k.s), k.b)
      invFun := fun p => ⟨p.1.1, p.1.2.1, p.2, p.1.2.2.1,
        p.1.2.2.2.1, p.1.2.2.2.2.1, p.1.2.2.2.2.2⟩
      left_inv := fun _ => rfl
      right_inv := fun _ => rfl }

lemma short_bound_for_coordinate [Fintype F] (hF : Fintype.card F = 2 ^ 128)
    (ι : R →+ F) (m m' : List Byte) (hm : m.length < 32) (hm' : m'.length < 32)
    (useA : Bool)
    (hc : (if useA then (shortField ι m).1 + (shortField ι m').1
      else (shortField ι m).2 + (shortField ι m').2) ≠ 0) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ 1 / 2 ^ 128 := by
  let e := hashKeyShortEquiv F useA
  let c := if useA then (shortField ι m).1 + (shortField ι m').1
      else (shortField ι m).2 + (shortField ι m').2
  let base := fun a => raijuhash ι (e.symm (a, 0)) m + raijuhash ι (e.symm (a, 0)) m'
  have haffine (k : HashKey F) :
      raijuhash ι k m + raijuhash ι k m' = base (e k).1 + c * (e k).2 := by
    simp only [base, raijuhash_short ι m hm, raijuhash_short ι m' hm']
    cases useA <;> simp [e, c, hashKeyShortEquiv] <;> ring
  have h := uniform_coordinate_bound e
    (fun k => raijuhash ι k m + raijuhash ι k m') base c hc haffine d
  have hcard : Nat.card F = 2 ^ 128 := (Nat.card_eq_fintype_card).trans hF
  simpa only [hcard, Nat.cast_pow, Nat.cast_ofNat] using h

lemma case_both_short [Fintype F] (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (m m' : List Byte) (hne : m ≠ m') (hm : m.length < 32) (hm' : m'.length < 32) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ 1 / 2 ^ 128 := by
  have he : shortField ι m ≠ shortField ι m' := fun h => hne (shortField_injective ι hι hm hm' h)
  by_cases ha : (shortField ι m).1 = (shortField ι m').1
  · have hb : (shortField ι m).2 ≠ (shortField ι m').2 := fun h => he (Prod.ext ha h)
    exact short_bound_for_coordinate hF ι m m' hm hm' false
      (by intro hz; exact hb (CharTwo.add_eq_zero.mp hz)) d
  · exact short_bound_for_coordinate hF ι m m' hm hm' true
      (by intro hz; exact ha (CharTwo.add_eq_zero.mp hz)) d

end RaijuHash.FullProof

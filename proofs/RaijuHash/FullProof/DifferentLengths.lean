import RaijuHash.FullProof.Basic
import RaijuHash.UniformCoordinate

namespace RaijuHash.FullProof
variable {F : Type} [Field F]

lemma lengthCoefficient_eq (ι : R →+ F) (m : List Byte) :
    lengthCoefficient ι m = ι (lenPoly (if m.length < 32 then 0 else m.length)) := by
  by_cases h : m.length < 32 <;> simp [lengthCoefficient, h, lenPoly, bitPoly_zero]

lemma lengthCoefficient_diff_ne [CharP F 2] (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (m m' : List Byte) (hlen : m.length ≠ m'.length)
    (hlong : 32 ≤ m.length ∨ 32 ≤ m'.length)
    (h64 : m.length < 2 ^ 64) (h64' : m'.length < 2 ^ 64) :
    lengthCoefficient ι m + lengthCoefficient ι m' ≠ 0 := by
  intro h
  rw [lengthCoefficient_eq, lengthCoefficient_eq] at h
  have he := ι_lenPoly_injective ι hι
    (a := if m.length < 32 then 0 else m.length)
    (b := if m'.length < 32 then 0 else m'.length)
    (by split_ifs <;> omega) (by split_ifs <;> omega) (CharTwo.add_eq_zero.mp h)
  split_ifs at he <;> omega

/-- Different lengths, with at least one long message. Short/short pairs use
    the independently keyed short encoding, not the length key. -/
lemma case_diff_length_long [CharP F 2] [Fintype F] (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0)
    (m m' : List Byte) (hlen : m.length ≠ m'.length)
    (hlong : 32 ≤ m.length ∨ 32 ≤ m'.length)
    (h64 : m.length < 2 ^ 64) (h64' : m'.length < 2 ^ 64) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ 1 / 2 ^ 128 := by
  let e := hashKeyTEquiv F
  let c := lengthCoefficient ι m + lengthCoefficient ι m'
  let base := fun a => raijuhash ι (e.symm (a, 0)) m + raijuhash ι (e.symm (a, 0)) m'
  have hc : c ≠ 0 := lengthCoefficient_diff_ne ι hι m m' hlen hlong h64 h64'
  have haffine (k : HashKey F) :
      raijuhash ι k m + raijuhash ι k m' = base (e k).1 + c * (e k).2 := by
    change raijuhash ι k m + raijuhash ι k m' =
      (raijuhash ι {k with t := 0} m + raijuhash ι {k with t := 0} m') +
        (lengthCoefficient ι m + lengthCoefficient ι m') * k.t
    have h₁ := raijuhash_change_t ι k m k.t
    have h₂ := raijuhash_change_t ι k m' k.t
    change raijuhash ι k m = _ at h₁
    change raijuhash ι k m' = _ at h₂
    rw [h₁, h₂]
    ring
  have h := uniform_coordinate_bound e
    (fun k => raijuhash ι k m + raijuhash ι k m') base c hc haffine d
  have hcard : Nat.card F = 2 ^ 128 := (Nat.card_eq_fintype_card).trans hF
  simpa only [hcard, Nat.cast_pow, Nat.cast_ofNat] using h

end RaijuHash.FullProof

import RaijuHash.FullProof.DifferentLengths
import RaijuHash.FullProof.Short
import RaijuHash.FullProof.Long

/-!
# Proof of Theorem 5: RaijuHash is `(⌈L/8192⌉ + 1) / 2¹²⁸`-AXU

For distinct messages of at most `L` bytes and any target `d`:
`Pr[H(m) ⊕ H(m') = d] ≤ (⌈L/8192⌉ + 1) / 2¹²⁸`.

## Outline (SPEC.md Theorem 5)

### Case 1: Different lengths, with at least one long message

`T` is independent of everything else. The output difference has a nonzero
coefficient on `T` (the lengths differ), so by the uniformity of `T` the
probability is exactly `2⁻¹²⁸`.

### Case 2: Both short, same or different length

The encodings `(X0, X1)` are injective on messages below 32 bytes (byte 15
records the length). Distinct encodings give a nonzero coefficient on `A` or
`B`, so the difference is uniform: `2⁻¹²⁸`.

### Case 3: Both long, equal length

Both have `q = ⌈L/8192⌉` chunks with equal chunk lengths. The outer
accumulator unrolled is a bivariate polynomial in `(R, R2)`:

```
P_q = ⊕_i [ h0_i · R^(q-i+1) + h1_i · R2 · R^(q-i) ]
```

a polynomial whose monomials are distinct and non-constant, of total degree
at most `q`. The variables `R, R2` are independent of the chain keys.

Condition on the chain keys:
- If some chunk pair differs: `P_q - P'_q - d` is a nonzero polynomial of
  degree `≤ q`, so by Schwartz–Zippel it vanishes with probability `≤ q / 2¹²⁸`.
- If all chunk pairs coincide: the first differing chunk must collide,
  probability `≤ 2⁻¹²⁸` by Proposition 4.

Total: `(q + 1) / 2¹²⁸`.
-/

namespace RaijuHash.FullProof

open Polynomial

variable {F : Type} [Field F] [Fintype F] [CharP F 2]

/-- **Different lengths.** Short/short uses the short encoding. Otherwise
    the length key has a nonzero coefficient and gives the counting bound. -/
lemma case_diff_length (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (m m' : List Byte) (hlen : m.length ≠ m'.length)
    (h64 : m.length < 2 ^ 64) (h64' : m'.length < 2 ^ 64) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ 1 / 2 ^ 128 := by
  by_cases hs : m.length < 32 ∧ m'.length < 32
  · exact case_both_short hF ι hι m m'
      (fun h => hlen (congrArg List.length h)) hs.1 hs.2 d
  · exact case_diff_length_long hF ι hι m m' hlen (by omega) h64 h64' d

/-- **Case 3: both long, equal length.** Schwartz–Zippel on the bivariate
    polynomial in `(R, R2)` of degree `≤ q`, plus Proposition 4 for the
    tables on which it vanishes identically (`card_both_long`). -/
lemma case_both_long (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (L : ℕ) (m m' : List Byte) (hm : m ≠ m')
    (hlen : m.length = m'.length) (hlong : 32 ≤ m.length) (hL : m.length ≤ L) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 := by
  have hpos : (0 : ℚ) < Nat.card (HashKey F) := by exact_mod_cast hashKey_card_pos F
  rw [div_le_div_iff₀ hpos (by positivity)]
  have hq := numChunks_le hL
  have h := (card_both_long hF ι hι m m' hm hlen hlong d).trans
    (Nat.mul_le_mul_right (Nat.card (HashKey F)) (Nat.add_le_add_right hq 1))
  exact_mod_cast h

/-- **Theorem 5.** RaijuHash is `(⌈L/8192⌉ + 1) / 2¹²⁸`-AXU. -/
theorem raijuhash_axu (F : Type) [Field F] [Fintype F] [CharP F 2]
    (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (L : ℕ) (m m' : List Byte) (hm : m ≠ m')
    (hL : m.length ≤ L) (hL' : m'.length ≤ L) (h64 : L < 2 ^ 64) (d : F) :
    (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
      / Nat.card (HashKey F) ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 := by
  by_cases hlen : m.length = m'.length
  · by_cases hlong : 32 ≤ m.length
    · -- Both long, equal length: Case 3.
      exact case_both_long hF ι hι L m m' hm hlen hlong hL d
    · push Not at hlong
      by_cases hlong' : 32 ≤ m'.length
      · omega -- impossible: lengths are equal
      · push Not at hlong'
        -- Both short: Case 2. The bound `1/2^128 ≤ (L/8192 + 1)/2^128`.
        calc (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
            / Nat.card (HashKey F)
            ≤ 1 / 2 ^ 128 := case_both_short hF ι hι m m' hm hlong hlong' d
          _ ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 := by
              apply div_le_div_of_nonneg_right _ (by positivity)
              have : (0 : ℚ) ≤ ((L + 8191) / 8192 : ℕ) := by positivity
              linarith
  · -- Different lengths: Case 1. The bound `1/2^128 ≤ (L/8192 + 1)/2^128`.
    calc (Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} : ℚ)
        / Nat.card (HashKey F)
        ≤ 1 / 2 ^ 128 := case_diff_length hF ι hι m m' hlen
            (hL.trans_lt h64) (hL'.trans_lt h64) d
      _ ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 := by
          apply div_le_div_of_nonneg_right _ (by positivity)
          have : (0 : ℚ) ≤ ((L + 8191) / 8192 : ℕ) := by positivity
          linarith

end RaijuHash.FullProof

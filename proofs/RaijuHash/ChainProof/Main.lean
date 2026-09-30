import RaijuHashCompress

/-!
# Proof of Lemma 2: chain code has distance ≥ 2

For distinct equal-length chunks that differ in at least one block, there is a
lane in which the encoded pairs differ at two or more positions.

## Outline (SPEC.md Lemma 2)

In a lane `l` where the blocks differ, let `d_b` be the per-block difference
`(c.blocks b l) - (c'.blocks b l)` for block `b`, and set `d_{-1} = d_n = 0`.
The encoded differences are `e_j = d_{j-1} ⊕ d_j` (each position encodes the
XOR of the current and previous block differences, because the key cancels).

The sequence `d_{-1}, d_0, ..., d_n` starts at 0, is not all zero (some block
differs in this lane), and ends at 0. A sequence that starts at 0, visits a
nonzero value, and returns to 0 must change value at least twice. Each change
at position `j` means `e_j ≠ 0`, i.e. the encoded pairs differ there.

Equal chunk lengths give equal `n` and identical zero-padding, so padding
creates no difference.
-/

namespace RaijuHash.ChainProof

open Polynomial

/-- A sequence `f : ℕ → α` that is zero at positions 0 and `n + 1` and nonzero
    somewhere in `1..n` changes value at least twice in `0..n + 1`.
    Each value change at `j` means `f j ≠ f (j + 1)`.

    More precisely: there exist `j₁ < j₂ ≤ n` such that
    `f j₁ ≠ f (j₁ + 1)` and `f j₂ ≠ f (j₂ + 1)`. -/
lemma changes_twice {α : Type} [Zero α] [DecidableEq α] (f : ℕ → α) (n : ℕ)
    (h0 : f 0 = 0) (hn : f (n + 1) = 0)
    (hne : ∃ k, 1 ≤ k ∧ k ≤ n ∧ f k ≠ 0) :
    ∃ j₁ j₂, j₁ < j₂ ∧ j₂ ≤ n ∧ f j₁ ≠ f (j₁ + 1) ∧ f j₂ ≠ f (j₂ + 1) := by
  rcases hne with ⟨k, hk1, hkn, hfk⟩
  have hS_nonempty : ∃ i, i ≤ n + 1 ∧ f i ≠ 0 := ⟨k, by omega, hfk⟩
  let j₁ := Nat.find hS_nonempty - 1
  have hj1_prop := Nat.find_spec hS_nonempty
  have hj1_pos : 0 < Nat.find hS_nonempty := by
    by_contra h
    push Not at h
    have : Nat.find hS_nonempty = 0 := by omega
    rw [this] at hj1_prop
    exact hj1_prop.2 h0
  have h_j1_1 : j₁ + 1 = Nat.find hS_nonempty := by omega
  have hj1_le_n : j₁ ≤ n := by
    have : Nat.find hS_nonempty ≤ n + 1 := hj1_prop.1
    omega
  have hfj1 : f j₁ = 0 := by
    by_contra h
    have : j₁ < Nat.find hS_nonempty := by omega
    exact Nat.find_min hS_nonempty this ⟨by omega, h⟩
  have hfj1_next : f (j₁ + 1) ≠ 0 := by
    rw [h_j1_1]
    exact hj1_prop.2

  have hS'_nonempty : ∃ i, Nat.find hS_nonempty ≤ i ∧ i ≤ n + 1 ∧ f i = 0 := ⟨n + 1, by omega, le_refl _, hn⟩
  let j₂ := Nat.find hS'_nonempty - 1
  have hj2_prop := Nat.find_spec hS'_nonempty
  have hj2_pos : Nat.find hS_nonempty < Nat.find hS'_nonempty := by
    by_contra h
    push Not at h
    have : Nat.find hS'_nonempty = Nat.find hS_nonempty := by omega
    rw [this] at hj2_prop
    exact hj1_prop.2 hj2_prop.2.2
  have h_j2_1 : j₂ + 1 = Nat.find hS'_nonempty := by omega
  have hj2_le_n : j₂ ≤ n := by
    have : Nat.find hS'_nonempty ≤ n + 1 := hj2_prop.2.1
    omega
  have hj1_lt_j2 : j₁ < j₂ := by omega
  have hfj2_next : f (j₂ + 1) = 0 := by
    rw [h_j2_1]
    exact hj2_prop.2.2
  have hfj2 : f j₂ ≠ 0 := by
    by_contra h
    have : j₂ < Nat.find hS'_nonempty := by omega
    have hj2_ge : Nat.find hS_nonempty ≤ j₂ := by omega
    exact Nat.find_min hS'_nonempty this ⟨hj2_ge, by omega, h⟩

  use j₁, j₂
  refine ⟨hj1_lt_j2, hj2_le_n, ?_, ?_⟩
  · intro h
    rw [hfj1] at h
    exact hfj1_next h.symm
  · intro h
    rw [hfj2_next] at h
    exact hfj2 h

/-- The per-block difference in lane `l`. Returns `(0, 0)` for `b` outside
    `0..n-1` (matching the convention that `w[-1] = w[n] = 0`). -/
noncomputable def blockDiff (c c' : Chunk) (l : Lane) (b : ℕ) : Word × Word :=
  if b < c.n then
    ((c.blocks b l).1 + (c'.blocks b l).1,
     (c.blocks b l).2 + (c'.blocks b l).2)
  else (0, 0)

/-- The encoded difference at position `j`: `e_j = d_{j-1} ⊕ d_j`.
    The key cancels because both chunks use the same key row. -/
noncomputable def encodedDiff (c c' : Chunk) (l : Lane) (j : ℕ) : Word × Word :=
  let dPrev := if j = 0 then (0, 0) else blockDiff c c' l (j - 1)
  let dCur := if j = c.n then (0, 0) else blockDiff c c' l j
  (dPrev.1 + dCur.1, dPrev.2 + dCur.2)

lemma add_left_cancel_char2 (x y : Word) (h : x + y = 0) : x = y := by
  have h1 : ((x : R) + (y : R)) = 0 := congrArg Subtype.val h
  apply Subtype.ext
  calc (x : R)
    _ = (x : R) + 0 := by ring
    _ = (x : R) + ((x : R) + (y : R)) := by rw [←h1]
    _ = (x : R) * 2 + (y : R) := by ring
    _ = (x : R) * 0 + (y : R) := by
      have h_two : (2 : R) = 0 := CharP.cast_eq_zero R 2
      rw [h_two]
    _ = (y : R) := by ring

/-- If blocks differ at position `b` in lane `l`, the block difference is nonzero
    there. -/
lemma blockDiff_ne_zero_of_ne (c c' : Chunk) (l : Lane) (b : ℕ) (hb : b < c.n)
    (hne : c.blocks b l ≠ c'.blocks b l) :
    blockDiff c c' l b ≠ (0, 0) := by
  simp only [blockDiff, hb, ite_true]
  intro h
  rw [Prod.ext_iff] at h
  apply hne
  rw [Prod.ext_iff]
  constructor
  · exact add_left_cancel_char2 _ _ h.1
  · exact add_left_cancel_char2 _ _ h.2

/-- If blocks differ at some position, there is a lane where they differ. -/
lemma exists_lane_diff (c c' : Chunk) (b : ℕ) (_hb : b < c.n)
    (hne : c.blocks b ≠ c'.blocks b) :
    ∃ l : Lane, c.blocks b l ≠ c'.blocks b l := by
  by_contra h
  push Not at h
  exact hne (funext h)

/-- **Lemma 2.** Chain code distance ≥ 2. -/
theorem chain_distance_two
    (c c' : Chunk) (hn : c.n = c'.n)
    (hdiff : ∃ b, b < c.n ∧ c.blocks b ≠ c'.blocks b)
    (K : KeyTable) :
    ∃ l : Lane, ∃ j₁ j₂ : ℕ, j₁ < j₂ ∧ j₂ ≤ c.n ∧
      encodedPair c K j₁ l ≠ encodedPair c' K j₁ l ∧
      encodedPair c K j₂ l ≠ encodedPair c' K j₂ l := by
  classical
  obtain ⟨b, hb, hne⟩ := hdiff
  obtain ⟨l, hl⟩ := exists_lane_diff c c' b hb hne
  let f : ℕ → R × R := fun i =>
    if i = 0 ∨ c.n < i then 0 else
      (((c.blocks (i - 1) l).1 : R) + (c'.blocks (i - 1) l).1,
       ((c.blocks (i - 1) l).2 : R) + (c'.blocks (i - 1) l).2)
  have hf : f (b + 1) ≠ 0 := by
    simp only [f, show ¬(b + 1 = 0 ∨ c.n < b + 1) by omega, ite_false,
      Nat.add_sub_cancel]
    intro h
    apply hl
    exact Prod.ext
      (Subtype.ext (CharTwo.add_eq_zero.mp (congrArg Prod.fst h)))
      (Subtype.ext (CharTwo.add_eq_zero.mp (congrArg Prod.snd h)))
  obtain ⟨j₁, j₂, hj, hj₂, h₁, h₂⟩ := changes_twice f c.n
    (by simp [f]) (by simp [f]) ⟨b + 1, by omega, by omega, hf⟩
  have bridge (j : ℕ) (hj : j ≤ c.n)
      (he : encodedPair c K j l = encodedPair c' K j l) : f j = f (j + 1) := by
    have he₁ := congrArg (fun p : Word × Word => (p.1 : R)) he
    have he₂ := congrArg (fun p : Word × Word => (p.2 : R)) he
    have htwo : (2 : R) = 0 := CharP.cast_eq_zero R 2
    by_cases hz : j = 0
    · subst j
      have hc : c.n ≠ 0 := by have := c.hn_pos; omega
      simp [encodedPair, encode, ← hn, Ne.symm hc] at he₁ he₂
      simp only [f, true_or, ite_true, zero_add,
        show ¬(1 = 0 ∨ c.n < 1) by have := c.hn_pos; omega, ite_false, Nat.sub_self]
      exact Prod.ext (by simpa [he₁] using (CharTwo.add_self_eq_zero ((c'.blocks 0 l).1 : R)).symm)
        (by simpa [he₂] using (CharTwo.add_self_eq_zero ((c'.blocks 0 l).2 : R)).symm)
    · by_cases heq : j = c.n
      · simp [encodedPair, encode, ← hn, ← heq, hz] at he₁ he₂
        simp only [f, hz, false_or, show ¬ c.n < j by omega, ite_false,
          show j + 1 ≠ 0 by omega, show c.n < j + 1 by omega, or_true, ite_true]
        exact Prod.ext (by simpa [he₁] using CharTwo.add_self_eq_zero ((c'.blocks (j - 1) l).1 : R))
          (by simpa [he₂] using CharTwo.add_self_eq_zero ((c'.blocks (j - 1) l).2 : R))
      · simp [encodedPair, encode, hz, heq, ← hn] at he₁ he₂
        simp only [f, hz, false_or, show ¬ c.n < j by omega, ite_false,
          show j + 1 ≠ 0 by omega, show ¬ c.n < j + 1 by omega, Nat.add_sub_cancel]
        apply Prod.ext
        · linear_combination he₁ + (((c'.blocks (j - 1) l).1 : R) - (c.blocks j l).1) * htwo
        · linear_combination he₂ + (((c'.blocks (j - 1) l).2 : R) - (c.blocks j l).2) * htwo
  exact ⟨l, j₁, j₂, hj, hj₂, fun h => h₁ (bridge j₁ (by omega) h),
    fun h => h₂ (bridge j₂ hj₂ h)⟩

end RaijuHash.ChainProof

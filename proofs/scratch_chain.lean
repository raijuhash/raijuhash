import Mathlib

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
    push_neg at h
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
    push_neg at h
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

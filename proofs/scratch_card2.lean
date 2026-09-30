import Mathlib
import RaijuHashCompress
import RaijuHash.NHProof.Main

namespace RaijuHash.NHProof

open Polynomial

lemma card_le_of_inj_proj2 (S : Set (Word × Word))
    (h_inj : ∀ b a₁ a₂, (a₁, b) ∈ S → (a₂, b) ∈ S → a₁ = a₂) :
    Nat.card S ≤ Nat.card Word := by
  let f : S → Word := fun x => x.val.2
  have hf : Function.Injective f := by
    intro ⟨⟨a1, b1⟩, h1⟩ ⟨⟨a2, b2⟩, h2⟩ heq
    dsimp [f] at heq
    have h_b : b1 = b2 := heq
    cases h_b
    have h_a : a1 = a2 := h_inj b1 a1 a2 h1 h2
    cases h_a
    rfl
  exact Nat.card_le_card_of_injective f hf

lemma card_le_of_inj_proj1 (S : Set (Word × Word))
    (h_inj : ∀ a b₁ b₂, (a, b₁) ∈ S → (a, b₂) ∈ S → b₁ = b₂) :
    Nat.card S ≤ Nat.card Word := by
  let f : S → Word := fun x => x.val.1
  have hf : Function.Injective f := by
    intro ⟨⟨a1, b1⟩, h1⟩ ⟨⟨a2, b2⟩, h2⟩ heq
    dsimp [f] at heq
    have h_a : a1 = a2 := heq
    cases h_a
    have h_b : b1 = b2 := h_inj a1 b1 b2 h1 h2
    cases h_b
    rfl
  exact Nat.card_le_card_of_injective f hf

lemma card_nh_le_proof (x y x' y' : Word) (hne : (x, y) ≠ (x', y'))
    {M : Type} [AddCommGroup M] (ι : R →+ M)
    (hι : ∀ z : R, z.degree < 128 → ι z = 0 → z = 0) (d : M) :
    Nat.card {ab : Word × Word //
      ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d}
      * 2 ^ 64 ≤ Nat.card (Word × Word) := by
  have H (a b : Word) : (ι (clmul (x + a) (y + b)) + ι (clmul (x' + a) (y' + b)) = d) ↔
      (ι (clmul (x + a) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y')) = d) := by
    rw [← ι.map_add]
    rw [nh_diff_affine x y x' y' a b]
  sorry

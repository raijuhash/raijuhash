import Mathlib
import RaijuHashCompress

namespace RaijuHash.NHProof

open Polynomial

instance : Finite (Word : Type) :=
  Finite.of_equiv (Fin 64 → ZMod 2) (Polynomial.degreeLTEquiv (ZMod 2) 64).toEquiv.symm

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

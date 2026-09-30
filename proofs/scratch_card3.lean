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
  let S := {ab : Word × Word | ι (clmul (x + ab.1) (y + ab.2)) + ι (clmul (x' + ab.1) (y' + ab.2)) = d}
  have h_bound : Nat.card S ≤ Nat.card Word := by
    rcases eq_or_ne y y' with hy_eq | hy_ne
    · -- case y = y'
      apply card_le_of_inj_proj1 S
      intro a b1 b2 h1 h2
      have hx_ne : (x : R) + (x' : R) ≠ 0 := by
        intro h
        apply hne
        apply Prod.ext _ hy_eq
        apply Subtype.ext
        have h_two : (2 : R) = 0 := CharP.cast_eq_zero R 2
        calc (x : R)
          _ = (x : R) + 0 := (add_zero _).symm
          _ = (x : R) + ((x : R) + (x' : R)) := by rw [h]
          _ = (x : R) + (x : R) + (x' : R) := (add_assoc _ _ _).symm
          _ = (x : R) * 2 + (x' : R) := by
            have h_two_x : (x : R) + (x : R) = (x : R) * 2 := (mul_two (x:R)).symm
            rw [h_two_x]
          _ = (x : R) * 0 + (x' : R) := by rw [h_two]
          _ = 0 + (x' : R) := by rw [mul_zero]
          _ = (x' : R) := zero_add _
      
      have h1' : ι (clmul (x + x') (y + b1)) = d := by
        have : ι (clmul (x + a) (y + b1)) + ι (clmul (x' + a) (y + b1)) = d := by
          have h1_sub : ι (clmul (x + a) (y + b1)) + ι (clmul (x' + a) (y' + b1)) = d := h1
          rw [hy_eq] at h1_sub
          exact h1_sub
        rw [← ι.map_add] at this
        have h_dist : (clmul (x + a) (y + b1) : R) + clmul (x' + a) (y + b1) = clmul (x + x') (y + b1) := by
          unfold clmul
          push_cast
          have h_add : ((x : R) + (a : R)) + ((x' : R) + (a : R)) = (x : R) + (x' : R) := by
            calc ((x:R) + (a:R)) + ((x':R) + (a:R))
              _ = (x:R) + (x':R) + ((a:R) + (a:R)) := by
                rw [add_assoc, add_comm (a:R) _, ← add_assoc, ← add_assoc (x:R)]
              _ = (x:R) + (x':R) + (a:R)*2 := by rw [← mul_two (a:R)]
              _ = (x:R) + (x':R) + (a:R)*0 := by rw [h_two]
              _ = (x:R) + (x':R) + 0 := by rw [mul_zero]
              _ = (x:R) + (x':R) := add_zero _
          rw [← add_mul, h_add]
        rw [h_dist] at this
        exact this
        
      have h2' : ι (clmul (x + x') (y + b2)) = d := by
        have : ι (clmul (x + a) (y + b2)) + ι (clmul (x' + a) (y + b2)) = d := by
          have h2_sub : ι (clmul (x + a) (y + b2)) + ι (clmul (x' + a) (y' + b2)) = d := h2
          rw [hy_eq] at h2_sub
          exact h2_sub
        rw [← ι.map_add] at this
        have h_dist : (clmul (x + a) (y + b2) : R) + clmul (x' + a) (y + b2) = clmul (x + x') (y + b2) := by
          unfold clmul
          push_cast
          have h_add : ((x : R) + (a : R)) + ((x' : R) + (a : R)) = (x : R) + (x' : R) := by
            calc ((x:R) + (a:R)) + ((x':R) + (a:R))
              _ = (x:R) + (x':R) + ((a:R) + (a:R)) := by
                rw [add_assoc, add_comm (a:R) _, ← add_assoc, ← add_assoc (x:R)]
              _ = (x:R) + (x':R) + (a:R)*2 := by rw [← mul_two (a:R)]
              _ = (x:R) + (x':R) + (a:R)*0 := by rw [h_two]
              _ = (x:R) + (x':R) + 0 := by rw [mul_zero]
              _ = (x:R) + (x':R) := add_zero _
          rw [← add_mul, h_add]
        rw [h_dist] at this
        exact this
      
      have heq : ι (clmul (x + x') (y + b1)) = ι (clmul (x + x') (y + b2)) := by rw [h1', h2']
      
      have heq4 : ι (((x : R) + (x' : R)) * ((b1 : R) - (b2 : R))) = 0 := by
        have h_sub : ι ((x + x' : R) * (y + b1 : R)) - ι ((x + x' : R) * (y + b2 : R)) = 0 := sub_eq_zero.mpr heq
        rw [← ι.map_sub] at h_sub
        have h_distrib : (x + x' : R) * (y + b1 : R) - (x + x' : R) * (y + b2 : R) = (x + x' : R) * ((b1 : R) - (b2 : R)) := by
          exact (mul_sub (x+x':R) (y+b1:R) (y+b2:R)).symm
        rw [h_distrib] at h_sub
        exact h_sub
        
      have hz : ((x : R) + (x' : R)) * ((b1 : R) - (b2 : R)) = 0 := by
        apply hι
        · apply degree_mul_lt_128
          · apply lt_of_le_of_lt (degree_add_le _ _)
            exact max_lt (degree_lt_64 x) (degree_lt_64 x')
          · apply lt_of_le_of_lt (degree_sub_le _ _)
            exact max_lt (degree_lt_64 b1) (degree_lt_64 b2)
        · exact heq4
      
      have hb_sub : (b1 : R) - (b2 : R) = 0 := by
        exact (mul_eq_zero.mp hz).resolve_left hx_ne
        
      apply Subtype.ext
      exact sub_eq_zero.mp hb_sub
    · -- case y ≠ y'
      apply card_le_of_inj_proj2 S
      intro b a1 a2 h1 h2
      have hdy : (y : R) + (y' : R) ≠ 0 := by
        intro h
        apply hy_ne
        apply Subtype.ext
        have h_two : (2 : R) = 0 := CharP.cast_eq_zero R 2
        calc (y : R)
          _ = (y : R) + 0 := (add_zero _).symm
          _ = (y : R) + ((y : R) + (y' : R)) := by rw [h]
          _ = (y : R) + (y : R) + (y' : R) := (add_assoc _ _ _).symm
          _ = (y : R) * 2 + (y' : R) := by
            have h_two_y : (y : R) + (y : R) = (y : R) * 2 := (mul_two (y:R)).symm
            rw [h_two_y]
          _ = (y : R) * 0 + (y' : R) := by rw [h_two]
          _ = 0 + (y' : R) := by rw [mul_zero]
          _ = (y' : R) := zero_add _
      
      have h1' : ι (clmul (x + a1) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y')) = d := by
        have : ι (clmul (x + a1) (y + b)) + ι (clmul (x' + a1) (y' + b)) = d := h1
        rw [← ι.map_add] at this
        rw [nh_diff_affine x y x' y' a1 b] at this
        exact this
      have h2' : ι (clmul (x + a2) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y')) = d := by
        have : ι (clmul (x + a2) (y + b)) + ι (clmul (x' + a2) (y' + b)) = d := h2
        rw [← ι.map_add] at this
        rw [nh_diff_affine x y x' y' a2 b] at this
        exact this
      
      have heq : ι (clmul (x + a1) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y')) =
                 ι (clmul (x + a2) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y')) := by
        rw [h1', h2']
      
      rw [ι.map_add, ι.map_add, ι.map_add, ι.map_add] at heq
      
      have heq2 : ι (clmul (x + a1) (y + y')) = ι (clmul (x + a2) (y + y')) := by
        exact add_right_cancel (add_right_cancel heq)
      
      unfold clmul at heq2
      have H1 : ((x : R) + (a1 : R)) * ((y : R) + (y' : R)) = (x : R) * ((y : R) + (y' : R)) + (a1 : R) * ((y : R) + (y' : R)) := add_mul (x:R) (a1:R) _
      have H2 : ((x : R) + (a2 : R)) * ((y : R) + (y' : R)) = (x : R) * ((y : R) + (y' : R)) + (a2 : R) * ((y : R) + (y' : R)) := add_mul (x:R) (a2:R) _
      rw [H1, H2] at heq2
      rw [ι.map_add, ι.map_add] at heq2
      have heq3 : ι ((a1 : R) * ((y : R) + (y' : R))) = ι ((a2 : R) * ((y : R) + (y' : R))) := by
        exact add_left_cancel heq2
      
      have heq4 : ι (((a1 : R) - (a2 : R)) * ((y : R) + (y' : R))) = 0 := by
        have h_sub : ι ((a1 : R) * ((y : R) + (y' : R))) - ι ((a2 : R) * ((y : R) + (y' : R))) = 0 := sub_eq_zero.mpr heq3
        rw [← ι.map_sub] at h_sub
        have h_distrib : (a1 : R) * ((y : R) + (y' : R)) - (a2 : R) * ((y : R) + (y' : R)) = ((a1 : R) - (a2 : R)) * ((y : R) + (y' : R)) := by
          exact (sub_mul (a1:R) (a2:R) _).symm
        rw [h_distrib] at h_sub
        exact h_sub
      
      have hz : ((a1 : R) - (a2 : R)) * ((y : R) + (y' : R)) = 0 := by
        apply hι
        · apply degree_mul_lt_128
          · apply lt_of_le_of_lt (degree_sub_le _ _)
            exact max_lt (degree_lt_64 a1) (degree_lt_64 a2)
          · apply lt_of_le_of_lt (degree_add_le _ _)
            exact max_lt (degree_lt_64 y) (degree_lt_64 y')
        · exact heq4
      
      have ha_sub : (a1 : R) - (a2 : R) = 0 := by
        exact (mul_eq_zero.mp hz).resolve_right hdy
      
      apply Subtype.ext
      exact sub_eq_zero.mp ha_sub
  calc Nat.card S * 2^64
    _ = Nat.card S * Nat.card Word := by rw [card_Word]
    _ ≤ Nat.card Word * Nat.card Word := Nat.mul_le_mul_right _ h_bound
    _ = Nat.card (Word × Word) := by rw [Nat.card_prod]

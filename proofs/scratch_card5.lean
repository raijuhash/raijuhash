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
    have h_two : (2 : R) = 0 := CharP.cast_eq_zero R 2
    rcases eq_or_ne y y' with hy_eq | hy_ne
    · apply card_le_of_inj_proj1 S
      intro a b1 b2 h1 h2
      have hx_ne : (x : R) + (x' : R) ≠ 0 := by
        intro h
        apply hne
        apply Prod.ext _ hy_eq
        apply Subtype.ext
        calc (x : R)
          _ = (x : R) + 0 := (add_zero _).symm
          _ = (x : R) + ((x : R) + (x' : R)) := by rw [h]
          _ = (x : R) + (x : R) + (x' : R) := (add_assoc _ _ _).symm
          _ = (x : R) * 2 + (x' : R) := by
            have h_two_x : (x : R) + (x : R) = (x : R) * 2 := (mul_two _).symm
            rw [h_two_x]
          _ = (x : R) * 0 + (x' : R) := by rw [h_two]
          _ = 0 + (x' : R) := by rw [mul_zero]
          _ = (x' : R) := zero_add _
          
      have h1_sub : ι (clmul (x + a) (y + b1)) + ι (clmul (x' + a) (y + b1)) = d := by
        have h_tmp : y' = y := hy_eq.symm
        have h_tmp2 : ι (clmul (x + a) (y + b1)) + ι (clmul (x' + a) (y' + b1)) = d := h1
        rw [h_tmp] at h_tmp2
        exact h_tmp2
      have h1_comb : ι (clmul (x + a) (y + b1) + clmul (x' + a) (y + b1)) = d := by
        have H := map_add ι (clmul (x + a) (y + b1) : R) (clmul (x' + a) (y + b1))
        rw [H]
        exact h1_sub
        
      have h2_sub : ι (clmul (x + a) (y + b2)) + ι (clmul (x' + a) (y + b2)) = d := by
        have h_tmp : y' = y := hy_eq.symm
        have h_tmp2 : ι (clmul (x + a) (y + b2)) + ι (clmul (x' + a) (y' + b2)) = d := h2
        rw [h_tmp] at h_tmp2
        exact h_tmp2
      have h2_comb : ι (clmul (x + a) (y + b2) + clmul (x' + a) (y + b2)) = d := by
        have H := map_add ι (clmul (x + a) (y + b2) : R) (clmul (x' + a) (y + b2))
        rw [H]
        exact h2_sub
        
      have heq_add1 : (clmul (x + a) (y + b1) : R) + clmul (x' + a) (y + b1) = clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b1 := nh_diff_affine x y x' y a b1
      have heq_add2 : (clmul (x + a) (y + b2) : R) + clmul (x' + a) (y + b2) = clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b2 := nh_diff_affine x y x' y a b2
      
      have h1_comb_2 : ι (clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b1) = d := by
        have H := congrArg ι heq_add1
        rw [← H]
        exact h1_comb
      have h2_comb_2 : ι (clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b2) = d := by
        have H := congrArg ι heq_add2
        rw [← H]
        exact h2_comb
        
      have heq : ι (clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b1) = ι (clmul (x + x') (y + y) + clmul a (y + y) + clmul (x + x') b2) := by rw [h1_comb_2, h2_comb_2]
      
      have h_cancel : (clmul (x + x') (y + y) : R) + clmul a (y + y) = 0 := by
        have H_y_add_y : (y : R) + (y : R) = 0 := by
          calc (y:R) + (y:R)
            _ = (y:R) * 2 := (mul_two _).symm
            _ = (y:R) * 0 := by rw [h_two]
            _ = 0 := mul_zero _
        unfold clmul
        push_cast
        calc ((x:R) + (x':R)) * ((y:R) + (y:R)) + (a:R) * ((y:R) + (y:R))
          _ = ((x:R) + (x':R)) * 0 + (a:R) * 0 := by rw [H_y_add_y]
          _ = 0 + 0 := by rw [mul_zero, mul_zero]
          _ = 0 := add_zero _
          
      have h1_simpl : (clmul (x + x') (y + y) : R) + clmul a (y + y) + clmul (x + x') b1 = clmul (x + x') b1 := by
        rw [h_cancel, zero_add]
      have h2_simpl : (clmul (x + x') (y + y) : R) + clmul a (y + y) + clmul (x + x') b2 = clmul (x + x') b2 := by
        rw [h_cancel, zero_add]
        
      have h1_comb_3 : ι (clmul (x + x') b1) = d := by
        have H := congrArg ι h1_simpl
        rw [← H]
        exact h1_comb_2
      have h2_comb_3 : ι (clmul (x + x') b2) = d := by
        have H := congrArg ι h2_simpl
        rw [← H]
        exact h2_comb_2
        
      have heq2 : ι (clmul (x + x') b1) = ι (clmul (x + x') b2) := by rw [h1_comb_3, h2_comb_3]
      
      have heq4 : ι (((x : R) + (x' : R)) * ((b1 : R) - (b2 : R))) = 0 := by
        have h_sub : ι ((x + x' : R) * (b1 : R)) - ι ((x + x' : R) * (b2 : R)) = 0 := sub_eq_zero.mpr heq2
        rw [← ι.map_sub] at h_sub
        have h_distrib : (x + x' : R) * (b1 : R) - (x + x' : R) * (b2 : R) = (x + x' : R) * ((b1 : R) - (b2 : R)) := (mul_sub _ _ _).symm
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
      
      have hb_sub : (b1 : R) - (b2 : R) = 0 := (mul_eq_zero.mp hz).resolve_left hx_ne
      apply Subtype.ext
      exact sub_eq_zero.mp hb_sub
      
    · -- case y ≠ y'
      apply card_le_of_inj_proj2 S
      intro b a1 a2 h1 h2
      have hdy : (y : R) + (y' : R) ≠ 0 := by
        intro h
        apply hy_ne
        apply Subtype.ext
        calc (y : R)
          _ = (y : R) + 0 := (add_zero _).symm
          _ = (y : R) + ((y : R) + (y' : R)) := by rw [h]
          _ = (y : R) + (y : R) + (y' : R) := (add_assoc _ _ _).symm
          _ = (y : R) * 2 + (y' : R) := by
            have h_two_y : (y : R) + (y : R) = (y : R) * 2 := (mul_two _).symm
            rw [h_two_y]
          _ = (y : R) * 0 + (y' : R) := by rw [h_two]
          _ = 0 + (y' : R) := by rw [mul_zero]
          _ = (y' : R) := zero_add _
          
      have h1_comb : ι (clmul (x + a1) (y + b) + clmul (x' + a1) (y' + b)) = d := by
        have H := map_add ι (clmul (x + a1) (y + b) : R) (clmul (x' + a1) (y' + b))
        rw [H]
        exact h1
      have heq_add1 : (clmul (x + a1) (y + b) : R) + clmul (x' + a1) (y' + b) = clmul (x + x') (y + y') + clmul a1 (y + y') + clmul (x + x') b := nh_diff_affine x y x' y' a1 b
      have h1_comb_2 : ι (clmul (x + x') (y + y') + clmul a1 (y + y') + clmul (x + x') b) = d := by
        have H := congrArg ι heq_add1
        rw [← H]
        exact h1_comb
        
      have h2_comb : ι (clmul (x + a2) (y + b) + clmul (x' + a2) (y' + b)) = d := by
        have H := map_add ι (clmul (x + a2) (y + b) : R) (clmul (x' + a2) (y' + b))
        rw [H]
        exact h2
      have heq_add2 : (clmul (x + a2) (y + b) : R) + clmul (x' + a2) (y' + b) = clmul (x + x') (y + y') + clmul a2 (y + y') + clmul (x + x') b := nh_diff_affine x y x' y' a2 b
      have h2_comb_2 : ι (clmul (x + x') (y + y') + clmul a2 (y + y') + clmul (x + x') b) = d := by
        have H := congrArg ι heq_add2
        rw [← H]
        exact h2_comb
        
      have heq : ι (clmul (x + x') (y + y') + clmul a1 (y + y') + clmul (x + x') b) = ι (clmul (x + x') (y + y') + clmul a2 (y + y') + clmul (x + x') b) := by rw [h1_comb_2, h2_comb_2]
      
      have h_assoc1 : (clmul (x + x') (y + y') : R) + clmul a1 (y + y') + clmul (x + x') b = (clmul (x + x') (y + y') : R) + (clmul a1 (y + y') + clmul (x + x') b) := add_assoc _ _ _
      have h_assoc2 : (clmul (x + x') (y + y') : R) + clmul a2 (y + y') + clmul (x + x') b = (clmul (x + x') (y + y') : R) + (clmul a2 (y + y') + clmul (x + x') b) := add_assoc _ _ _
      
      have h_eq_step1 : ι ((clmul (x + x') (y + y') : R) + (clmul a1 (y + y') + clmul (x + x') b)) = ι ((clmul (x + x') (y + y') : R) + (clmul a2 (y + y') + clmul (x + x') b)) := by
        have H1 := congrArg ι h_assoc1
        have H2 := congrArg ι h_assoc2
        rw [← H1, ← H2]
        exact heq
        
      have H_map1 := map_add ι (clmul (x + x') (y + y') : R) (clmul a1 (y + y') + clmul (x + x') b)
      have H_map2 := map_add ι (clmul (x + x') (y + y') : R) (clmul a2 (y + y') + clmul (x + x') b)
      rw [H_map1, H_map2] at h_eq_step1
      have h_eq_step2 : ι (clmul a1 (y + y') + clmul (x + x') b) = ι (clmul a2 (y + y') + clmul (x + x') b) := add_left_cancel h_eq_step1
      
      have H_map3 := map_add ι (clmul a1 (y + y') : R) (clmul (x + x') b)
      have H_map4 := map_add ι (clmul a2 (y + y') : R) (clmul (x + x') b)
      rw [H_map3, H_map4] at h_eq_step2
      have heq_final : ι (clmul a1 (y + y')) = ι (clmul a2 (y + y')) := add_right_cancel h_eq_step2
      
      have heq4 : ι (((a1 : R) - (a2 : R)) * ((y : R) + (y' : R))) = 0 := by
        have h_sub : ι ((a1 : R) * ((y : R) + (y' : R))) - ι ((a2 : R) * ((y : R) + (y' : R))) = 0 := sub_eq_zero.mpr heq_final
        rw [← ι.map_sub] at h_sub
        have h_distrib : (a1 : R) * ((y : R) + (y' : R)) - (a2 : R) * ((y : R) + (y' : R)) = ((a1 : R) - (a2 : R)) * ((y : R) + (y' : R)) := (sub_mul _ _ _).symm
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
      
      have ha_sub : (a1 : R) - (a2 : R) = 0 := (mul_eq_zero.mp hz).resolve_right hdy
      apply Subtype.ext
      exact sub_eq_zero.mp ha_sub
      
  calc Nat.card S * 2^64
    _ = Nat.card S * Nat.card Word := by rw [card_Word]
    _ ≤ Nat.card Word * Nat.card Word := Nat.mul_le_mul_right _ h_bound
    _ = Nat.card (Word × Word) := by rw [Nat.card_prod]

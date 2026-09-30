import Mathlib
import RaijuHashCompress

namespace RaijuHash

open Polynomial

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

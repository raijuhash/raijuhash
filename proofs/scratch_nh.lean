import Mathlib
import RaijuHashCompress

namespace RaijuHash.NHProof

open Polynomial

lemma nh_diff_affine (x y x' y' a b : Word) :
    (clmul (x + a) (y + b) : R) + clmul (x' + a) (y' + b)
      = clmul (x + a) (y + y') + clmul (x + x') (y + b) + clmul (x + x') (y + y') := by
  have h_two : (2 : R) = 0 := CharP.cast_eq_zero R 2
  unfold clmul
  push_cast
  calc
    ((x : R) + (a : R)) * ((y : R) + (b : R)) + ((x' : R) + (a : R)) * ((y' : R) + (b : R))
      = (((x : R) + (a : R)) * ((y : R) + (y' : R)) + ((x : R) + (x' : R)) * ((y : R) + (b : R)) + ((x : R) + (x' : R)) * ((y : R) + (y' : R))) +
        ((a : R) * (b : R) - (x : R) * (y : R) - (x : R) * (y' : R) - (x' : R) * (y : R)) * 2 := by ring
    _ = (((x : R) + (a : R)) * ((y : R) + (y' : R)) + ((x : R) + (x' : R)) * ((y : R) + (b : R)) + ((x : R) + (x' : R)) * ((y : R) + (y' : R))) +
        ((a : R) * (b : R) - (x : R) * (y : R) - (x : R) * (y' : R) - (x' : R) * (y : R)) * 0 := by rw [h_two]
    _ = (((x : R) + (a : R)) * ((y : R) + (y' : R)) + ((x : R) + (x' : R)) * ((y : R) + (b : R)) + ((x : R) + (x' : R)) * ((y : R) + (y' : R))) := by ring

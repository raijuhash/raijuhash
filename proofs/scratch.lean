import Mathlib

abbrev R := (ZMod 2)[X]

lemma test (x y x' y' a b : R) :
  (x + a) * (y + b) + (x' + a) * (y' + b) =
  (x + a) * (y + y') + (x + x') * (y + b) + (x + x') * (y + y') := by
  ring

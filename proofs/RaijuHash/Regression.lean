import RaijuHash.ShortEncoding
import RaijuHash.FullProof.Basic

namespace RaijuHash.Regression
open Polynomial

/-- Natural-number casts collapse the old `(1,j)` columns in characteristic 2. -/
theorem nat_cast_columns_collapse {F : Type} [Field F] [CharP F 2] :
    (2 : F) = (0 : F) := CharP.cast_eq_zero F 2

/-- The corrected bit-polynomial columns distinguish positions 0 and 2. -/
theorem bit_columns_distinct {F : Type} [Field F] (ι : R →+ F)
    (hι : ∀ p : R, p.degree < 128 → ι p = 0 → p = 0) :
    ι (bitPoly 6 0) ≠ ι (bitPoly 6 2) := by
  intro h
  have := map_bitPoly_injective ι hι (by norm_num)
    (by norm_num : 0 < 2 ^ 6) (by norm_num : 2 < 2 ^ 6) h
  omega

/-- Zero padding cannot erase the length marker of a one-byte zero message. -/
theorem empty_ne_zero_byte_encoding :
    bytes128 ([] : List Byte) + bitPoly 8 0 * X ^ 120 ≠
      bytes128 [0] + bitPoly 8 1 * X ^ 120 := by
  intro h
  have := taggedPoly_length_injective [] [0] (by simp) (by simp)
    0 1 (by norm_num) (by norm_num) h
  omega

/-- At the first byte beyond a chunk boundary the ceiling increases. -/
theorem chunk_boundaries :
    numChunks (List.replicate 0 (0 : Byte)) = 0 ∧
    numChunks (List.replicate 1 (0 : Byte)) = 1 ∧
    numChunks (List.replicate 8192 (0 : Byte)) = 1 ∧
    numChunks (List.replicate 8193 (0 : Byte)) = 2 := by
  simp only [FullProof.numChunks_eq, List.length_replicate]
  norm_num

/-- The old unrestricted constructor would imply an impossible nonempty
    chunk at index zero of an empty message. -/
theorem empty_has_no_chunk : IsEmpty (Fin (numChunks ([] : List Byte))) := by
  change IsEmpty (Fin 0)
  infer_instance

end RaijuHash.Regression

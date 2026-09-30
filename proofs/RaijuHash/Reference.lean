import Mathlib

/-!
# Transcription of `crates/raijuhash/src/reference.rs`

A line-by-line transcription of the Rust reference implementation into
computable Lean, with Rust's integer semantics:

* `u8`, `u64`, `u128` values are natural numbers below `2⁸`, `2⁶⁴`, `2¹²⁸`;
* `^` is `^^^`, `>>` is `>>>`, `&` is `&&&`;
* `u128 << k` discards the bits shifted past bit 127 (`shl128`);
* `x as u64` is `x % 2⁶⁴`, `x as u8` is `x % 2⁸`;
* a byte slice is a `List ℕ`, and `from_le_bytes` is `fromLE`.

`RaijuHash/ReferenceProof.lean` proves that `hash` computes the formal
`raijuhash` over `GF128`. This transcription is checked against the Rust
code by the frozen vectors in `RaijuHash/ReferenceVectors.lean`, which are
the same vectors `crates/raijuhash/tests/vectors.rs` checks every Rust
backend against. The transcription itself is compared with the Rust source
by reading, not by a verified compiler.
-/

namespace RaijuHash.Reference

/-- `u128 << k`: bits shifted past bit 127 are discarded. -/
def shl128 (x k : ℕ) : ℕ := (x <<< k) % 2 ^ 128

/-- `clmul64`: `for i in 0..64 { if (b >> i) & 1 == 1 { r ^= (a as u128) << i; } }`. -/
def clmul64 (a b : ℕ) : ℕ :=
  (List.range 64).foldl (fun r i => if (b >>> i) &&& 1 = 1 then r ^^^ shl128 a i else r) 0

/-- The `fold` closure of `reduce256`. -/
def reduceFold (h : ℕ) : ℕ × ℕ :=
  (h ^^^ shl128 h 1 ^^^ shl128 h 2 ^^^ shl128 h 7,
    (h >>> 127) ^^^ (h >>> 126) ^^^ (h >>> 121))

/-- `reduce256` (Rust's `over` is `hiBits`; `over` is a Lean keyword). -/
def reduce256 (low high : ℕ) : ℕ :=
  let (t, hiBits) := reduceFold high
  let (t2, _hiBits2) := reduceFold hiBits
  low ^^^ t ^^^ t2

/-- `gf_mul`. -/
def gfMul (a b : ℕ) : ℕ :=
  let a0 := a % 2 ^ 64
  let a1 := (a >>> 64) % 2 ^ 64
  let b0 := b % 2 ^ 64
  let b1 := (b >>> 64) % 2 ^ 64
  let lo := clmul64 a0 b0
  let hi := clmul64 a1 b1
  let mid := clmul64 a0 b1 ^^^ clmul64 a1 b0
  let low := lo ^^^ shl128 mid 64
  let high := hi ^^^ (mid >>> 64)
  reduce256 low high

/-- `u64::from_le_bytes` and `u128::from_le_bytes`. -/
def fromLE (bs : List ℕ) : ℕ := bs.foldr (fun b acc => b + 256 * acc) 0

/-- `word(bytes, i)`: `u64::from_le_bytes(bytes[8 * i..8 * i + 8])`. -/
def word (bytes : List ℕ) (i : ℕ) : ℕ := fromLE ((bytes.drop (8 * i)).take 8)

/-- `Params`: `table: [[u64; 16]; 65]` and the field elements. -/
structure Params where
  table : Fin 65 → Fin 16 → ℕ
  a : ℕ
  b : ℕ
  r : ℕ
  r2 : ℕ
  t : ℕ
  s : ℕ
  v : ℕ

/-- `p.table[j][i]`; `chunk_pair` only indexes `j ≤ 64`, `i < 16`. -/
def Params.row (p : Params) (j i : ℕ) : ℕ :=
  if h : j < 65 ∧ i < 16 then p.table ⟨j, h.1⟩ ⟨i, h.2⟩ else 0

/-- Every field has the width of its Rust type. -/
def Params.WellFormed (p : Params) : Prop :=
  (∀ j i, p.table j i < 2 ^ 64) ∧ p.a < 2 ^ 128 ∧ p.b < 2 ^ 128 ∧ p.r < 2 ^ 128 ∧
    p.r2 < 2 ^ 128 ∧ p.t < 2 ^ 128 ∧ p.s < 2 ^ 128 ∧ p.v < 2 ^ 128

/-- `TABLE_BYTES`. -/
def tableBytes : ℕ := 65 * 128

/-- `Params::from_bytes`, with the key given as a function on byte indices. -/
def Params.fromBytes (kb : ℕ → ℕ) : Params :=
  let f : ℕ → ℕ := fun i => fromLE ((List.range 16).map fun k => kb (tableBytes + 16 * i + k))
  { table := fun j i => fromLE ((List.range 8).map fun k => kb (128 * j.val + 8 * i.val + k))
    a := f 0, b := f 1, r := f 2, r2 := f 3, t := f 4, s := f 5, v := f 6 }

/-- `chunk_pair`, for a chunk of 1..=8192 bytes. -/
def chunkPair (p : Params) (chunk : List ℕ) : ℕ × ℕ :=
  let nb := (chunk.length + 127) / 128
  -- `blocks[b]`: block `b` of the chunk, zero-padded to 128 bytes.
  let blocks : ℕ → List ℕ := fun b => (List.range 128).map fun k => chunk.getD (128 * b + k) 0
  let w : ℕ → ℕ → ℕ := fun b l => if b < nb then word (blocks b) l else 0
  (List.range (nb + 1)).foldl (fun (h : ℕ × ℕ) j =>
    let pj := (List.range 8).foldl (fun pj l =>
      let prev : ℕ → ℕ := fun i => if j > 0 then w (j - 1) i else 0
      let tx := prev l ^^^ w j l ^^^ p.row j l
      let ty := prev (8 + l) ^^^ w j (8 + l) ^^^ p.row j (8 + l)
      pj ^^^ clmul64 tx ty) 0
    if j = 64 then (h.1, h.2 ^^^ pj) else (h.1 ^^^ pj, h.2 ^^^ gfMul j pj)) (0, 0)

/-- `[0u8; 16]` with `bs` copied to its start. -/
def pad16 (bs : List ℕ) : List ℕ := (List.range 16).map fun k => bs.getD k 0

/-- `msg.chunks(CHUNK)`: consecutive 8192-byte slices, the last one shorter. -/
def chunks (msg : List ℕ) : List (List ℕ) :=
  (List.range ((msg.length + 8191) / 8192)).map fun i => (msg.drop (8192 * i)).take 8192

/-- `hash`. -/
def hash (p : Params) (msg : List ℕ) : ℕ :=
  let len := msg.length
  if len < 16 then
    let x0 := (pad16 msg).set 15 (len % 256)
    gfMul (fromLE x0) p.a ^^^ p.s
  else if len < 32 then
    let x0 := fromLE (msg.take 16)
    let x1 := (pad16 (msg.drop 16)).set 15 (len % 256)
    gfMul x0 p.a ^^^ gfMul (fromLE x1) p.b ^^^ p.s
  else
    let acc := (chunks msg).foldl (fun acc chunk =>
      let h := chunkPair p chunk
      gfMul (acc ^^^ h.1) p.r ^^^ gfMul h.2 p.r2) 0
    acc ^^^ gfMul (len % 2 ^ 64) p.t ^^^ p.s

end RaijuHash.Reference

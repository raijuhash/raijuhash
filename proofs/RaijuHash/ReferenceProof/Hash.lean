import RaijuHash.ReferenceProof.Chunk

/-!
# `hash` computes `raijuhash`

The message-level part of the refinement: Rust's chunk slices have the
model's blocks (`chunkRel_msg`), the short path builds the model's tagged
encodings (`toPoly_tagged`, `toPoly_first16`), and the outer loop is the
Horner accumulator. `hash_spec` combines them.
-/

set_option maxRecDepth 4096

namespace RaijuHash.Reference
open Polynomial RaijuHash

/-! ### Messages -/

/-- A Rust byte slice as a model message. -/
def msgOf (msg : List ℕ) : List Byte := msg.map byteOf

lemma byteOf_zero : byteOf 0 = 0 := by
  funext i
  simp [byteOf]

lemma byteAt_msgOf (msg : List ℕ) (n : ℕ) : byteAt (msgOf msg) n = byteOf (msg.getD n 0) := by
  unfold byteAt msgOf
  rw [← byteOf_zero, List.getD_map]

lemma getD_lt_256 {bs : List ℕ} (hb : ∀ b ∈ bs, b < 256) (n : ℕ) : bs.getD n 0 < 256 := by
  rw [List.getD_eq_getElem?_getD]
  cases h : bs[n]? with
  | none => simp
  | some y => exact hb y (List.mem_of_getElem? h)

/-- A polynomial of degree below `8w` whose coefficient `k` is bit `k % 8` of
    byte `k / 8` of `bs` is `toPoly (fromLE bs)`. -/
lemma toPoly_fromLE_of_coeff (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) (P : R)
    (hP : ∀ k, P.coeff k = if (bs.getD (k / 8) 0).testBit (k % 8) then 1 else 0) :
    toPoly (fromLE bs) = P := by
  symm
  apply toPoly_ext
  intro k
  rw [hP, testBit_fromLE bs hb]

lemma wordAt_coeff (m : List Byte) (n k : ℕ) :
    (wordAt m n : R).coeff k =
      if _ : k < 64 then byteAt m (8 * n + k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩ else 0 := by
  by_cases h : k < 64
  · simp only [h, dite_true]
    exact congrFun ((degreeLTEquiv (ZMod 2) 64).apply_symm_apply
      (fun k => byteAt m (8 * n + k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩)) ⟨k, h⟩
  · simp only [h, dite_false]
    exact coeff_eq_zero_of_degree_lt ((mem_degreeLT.mp (wordAt m n).2).trans_le
      (by exact_mod_cast not_lt.mp h))

lemma bytes128_coeff' (m : List Byte) (k : ℕ) :
    (bytes128 m).coeff k =
      if _ : k < 128 then byteAt m (k / 8) ⟨k % 8, Nat.mod_lt _ (by norm_num)⟩ else 0 := by
  by_cases h : k < 128
  · simp only [h, dite_true]
    exact bytes128_coeff m ⟨k, h⟩
  · simp only [h, dite_false]
    exact coeff_eq_zero_of_degree_lt ((bytes128_degree m).trans_le
      (by exact_mod_cast not_lt.mp h))

lemma byteOf_bit (n : ℕ) (k : ℕ) (hk : k < 8) :
    byteOf n ⟨k, hk⟩ = if n.testBit k then 1 else 0 := rfl

lemma getD_take_drop (l : List ℕ) (a n j : ℕ) :
    ((l.drop a).take n).getD j 0 = if j < n then l.getD (a + j) 0 else 0 := by
  simp only [List.getD_eq_getElem?_getD, List.getElem?_take, List.getElem?_drop]
  split_ifs <;> simp

lemma getD_blockOf (cs : List ℕ) (b j : ℕ) :
    (blockOf cs b).getD j 0 = if j < 128 then cs.getD (128 * b + j) 0 else 0 := by
  unfold blockOf
  simp only [List.getD_eq_getElem?_getD, List.getElem?_map]
  split_ifs <;> simp_all

lemma testBit_of_lt_two_pow {w n k : ℕ} (h : w < 2 ^ n) (hk : n ≤ k) : w.testBit k = false :=
  Nat.testBit_lt_two_pow (h.trans_le (Nat.pow_le_pow_right (by norm_num) hk))

/-- The Rust chunk slice `msg.chunks(CHUNK)[i]`. -/
def slice (msg : List ℕ) (i : ℕ) : List ℕ := (msg.drop (8192 * i)).take 8192

lemma chunks_eq (msg : List ℕ) :
    chunks msg = (List.range (numChunks (msgOf msg))).map (slice msg) := by
  rw [chunks, FullProof.numChunks_eq, msgOf, List.length_map]
  rfl

lemma wordAt_msg (msg : List ℕ) (hb : ∀ b ∈ msg, b < 256) (i b l : ℕ) (hb64 : b < 64)
    (hl : l < 16) :
    wordAt (msgOf msg) (16 * (64 * i + b) + l) = toWord (word (blockOf (slice msg i) b) l) := by
  have hsl : ∀ x ∈ slice msg i, x < 256 :=
    fun x hx => hb x (List.mem_of_mem_drop (List.mem_of_mem_take hx))
  have hw := word_lt _ (blockOf_bytes _ hsl b) l
  apply Subtype.ext
  rw [toWord_coe hw]
  apply toPoly_ext
  intro k
  rw [wordAt_coeff]
  by_cases hk : k < 64
  · simp only [hk, dite_true, byteAt_msgOf, byteOf_bit]
    unfold word
    rw [testBit_fromLE _ (fun x hx => blockOf_bytes _ hsl b x
      (List.mem_of_mem_drop (List.mem_of_mem_take hx)))]
    have e1 : (((blockOf (slice msg i) b).drop (8 * l)).take 8).getD (k / 8) 0 =
        msg.getD (8 * (16 * (64 * i + b) + l) + k / 8) 0 := by
      rw [getD_take_drop, ite_eq_left (show k / 8 < 8 by omega), getD_blockOf,
        ite_eq_left (show 8 * l + k / 8 < 128 by omega)]
      unfold slice
      rw [getD_take_drop, ite_eq_left (show 128 * b + (8 * l + k / 8) < 8192 by omega)]
      congr 1
      ring
    rw [e1]
  · simp only [hk, dite_false, testBit_of_lt_two_pow hw (not_lt.mp hk), Bool.false_eq_true,
      ite_false]

/-- The Rust chunk slices and the model's chunks have the same blocks. -/
lemma chunkRel_msg (msg : List ℕ) (hb : ∀ b ∈ msg, b < 256) (i : ℕ)
    (hi : i < numChunks (msgOf msg)) :
    ChunkRel (slice msg i) (msgChunk (msgOf msg) ⟨i, hi⟩) := by
  have hlen : (msgOf msg).length = msg.length := List.length_map _
  have hsl : ∀ x ∈ slice msg i, x < 256 :=
    fun x hx => hb x (List.mem_of_mem_drop (List.mem_of_mem_take hx))
  have hn : (msgChunk (msgOf msg) ⟨i, hi⟩).n = nbOf (slice msg i) := by
    have hs : (slice msg i).length = min 8192 (msg.length - 8192 * i) := by
      simp [slice]
    have hi' : i < ((msg.length + 127) / 128 + 63) / 64 := by
      simpa only [numChunks, numBlocks, hlen] using hi
    rw [FullProof.msgChunk_n, nbOf, hs, numBlocks, hlen]
    dsimp only
    omega
  refine ⟨hsl, hn, fun b hbn l => ?_⟩
  have hb64 : b < 64 := by have := (msgChunk (msgOf msg) ⟨i, hi⟩).hn_le; omega
  have hbn' : b < nbOf (slice msg i) := hn ▸ hbn
  change (wordAt (msgOf msg) (16 * (64 * i + b) + l.1),
    wordAt (msgOf msg) (16 * (64 * i + b) + 8 + l.1)) = _
  rw [wOf, wOf, ite_eq_left hbn', ite_eq_left hbn', wordAt_msg msg hb i b l.1 hb64 (by omega),
    show 16 * (64 * i + b) + 8 + l.1 = 16 * (64 * i + b) + (8 + l.1) by ring,
    wordAt_msg msg hb i b (8 + l.1) hb64 (by omega)]

/-! ### Short messages -/

lemma getD_pad16_set (bs : List ℕ) (n j : ℕ) :
    ((pad16 bs).set 15 n).getD j 0 =
      if j = 15 then n else if j < 16 then bs.getD j 0 else 0 := by
  unfold pad16
  simp only [List.getD_eq_getElem?_getD, List.getElem?_set, List.length_map, List.length_range,
    List.getElem?_map]
  by_cases h15 : j = 15
  · simp [h15]
  · by_cases h16 : j < 16
    · simp [h15, h16, Ne.symm h15]
    · simp [h15, h16, Ne.symm h15]

lemma pad16_bytes (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) (n : ℕ) (hn : n < 256) :
    ∀ x ∈ (pad16 bs).set 15 n, x < 256 := by
  intro x hx
  obtain ⟨j, hj, rfl⟩ := List.getElem_of_mem hx
  have := getD_pad16_set bs n j
  rw [List.getD_eq_getElem _ _ hj] at this
  rw [this]
  split_ifs
  · exact hn
  · exact getD_lt_256 hb j
  · norm_num

lemma bitPoly8_coeff {n : ℕ} (hn : n < 256) (j : ℕ) :
    (bitPoly 8 n).coeff j = if n.testBit j then 1 else 0 := by
  rw [bitPoly_eq_toPoly (by simpa using hn), toPoly_coeff]

/-- The tagged 16-byte block of the short path: `x[..len] = bs`, `x[15] = n`. -/
lemma toPoly_tagged (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) (hlen : bs.length ≤ 15)
    (n : ℕ) (hn : n < 256) :
    fromLE ((pad16 bs).set 15 n) < 2 ^ 128 ∧
      toPoly (fromLE ((pad16 bs).set 15 n)) = bytes128 (msgOf bs) + bitPoly 8 n * X ^ 120 := by
  have hx := pad16_bytes bs hb n hn
  refine ⟨(fromLE_lt _ hx).trans_le (by simp [pad16]), ?_⟩
  apply toPoly_fromLE_of_coeff _ hx
  intro k
  rw [coeff_add, coeff_mul_X_pow', bytes128_coeff', getD_pad16_set, bitPoly8_coeff hn]
  by_cases hk : k < 120
  · simp only [show k < 128 by omega, dite_true, show ¬ 120 ≤ k by omega, add_zero,
      show k / 8 ≠ 15 by omega, show k / 8 < 16 by omega, byteAt_msgOf, byteOf_bit, ↓reduceIte]
  · by_cases hk' : k < 128
    · have h15 : k / 8 = 15 := by omega
      have hz : bs.getD 15 0 = 0 := List.getD_eq_default _ _ (by omega)
      have h8 : k - 120 = k % 8 := by omega
      simp only [hk', dite_true, show 120 ≤ k by omega, h15, byteAt_msgOf, hz,
        byteOf_bit, Nat.zero_testBit, Bool.false_eq_true, zero_add, h8, ↓reduceIte]
    · simp only [hk', dite_false, show 120 ≤ k by omega, ite_true, zero_add,
        show k / 8 ≠ 15 by omega, show ¬ k / 8 < 16 by omega, ite_false, Nat.zero_testBit,
        Bool.false_eq_true, testBit_of_lt_two_pow (n := 8) (by simpa using hn) (by omega : 8 ≤ k - 120)]

/-- `u128::from_le_bytes(msg[..16])`. -/
lemma toPoly_first16 (msg : List ℕ) (hb : ∀ b ∈ msg, b < 256) :
    fromLE (msg.take 16) < 2 ^ 128 ∧ toPoly (fromLE (msg.take 16)) = bytes128 (msgOf msg) := by
  have hx : ∀ x ∈ msg.take 16, x < 256 := fun x hx => hb x (List.mem_of_mem_take hx)
  refine ⟨(fromLE_lt _ hx).trans_le (Nat.pow_le_pow_right (by norm_num) (by simp; omega)), ?_⟩
  apply toPoly_fromLE_of_coeff _ hx
  intro k
  rw [bytes128_coeff']
  have htake : (msg.take 16).getD (k / 8) 0 = if k / 8 < 16 then msg.getD (k / 8) 0 else 0 := by
    have := getD_take_drop msg 0 16 (k / 8)
    simpa using this
  rw [htake]
  by_cases hk : k < 128
  · simp only [hk, dite_true, show k / 8 < 16 by omega, ite_true, byteAt_msgOf, byteOf_bit]
  · simp only [hk, dite_false, show ¬ k / 8 < 16 by omega, ite_false, Nat.zero_testBit,
      Bool.false_eq_true]

/-! ### `hash` -/

lemma fe_zero : fe 0 = 0 := by simp [fe]

/-- The loop body of `hash` over chunks. -/
def outerStep (p : Params) (acc : ℕ) (chunk : List ℕ) : ℕ :=
  gfMul (acc ^^^ (chunkPair p chunk).1) p.r ^^^ gfMul (chunkPair p chunk).2 p.r2

lemma outer_fold (p : Params) (hp : p.WellFormed) (msg : List ℕ) (D : ℕ → GF128 × GF128)
    (xs : List ℕ)
    (hD : ∀ i ∈ xs, (chunkPair p (slice msg i)).1 < 2 ^ 128 ∧
      (chunkPair p (slice msg i)).2 < 2 ^ 128 ∧
      (fe (chunkPair p (slice msg i)).1, fe (chunkPair p (slice msg i)).2) = D i)
    (acc : ℕ) (hacc : acc < 2 ^ 128) :
    (xs.map (slice msg)).foldl (outerStep p) acc < 2 ^ 128 ∧
      fe ((xs.map (slice msg)).foldl (outerStep p) acc) =
        (xs.map D).foldl (fun a d => (a + d.1) * fe p.r + d.2 * fe p.r2) (fe acc) := by
  induction xs generalizing acc with
  | nil => exact ⟨hacc, rfl⟩
  | cons i xs ih =>
    obtain ⟨h1, h2, hd⟩ := hD i (by simp)
    have hr := hp.2.2.2.1
    have hr2 := hp.2.2.2.2.1
    have hx := Nat.xor_lt_two_pow hacc h1
    have hstep : outerStep p acc (slice msg i) < 2 ^ 128 :=
      Nat.xor_lt_two_pow (gfMul_spec hx hr).1 (gfMul_spec h2 hr2).1
    obtain ⟨hlt, heq⟩ := ih (fun j hj => hD j (by simp [hj])) _ hstep
    refine ⟨hlt, ?_⟩
    simp only [List.map_cons, List.foldl_cons]
    rw [heq]
    congr 1
    rw [outerStep, fe_xor, fe_gfMul hx hr, fe_gfMul h2 hr2, fe_xor, ← hd]

/-- **Refinement.** For well-formed parameters, byte-valued messages, and
    lengths below `2⁶⁴`, the transcription of `reference::hash` returns a
    `u128` whose field element is the formal `raijuhash` over `GF128`. -/
theorem hash_spec (p : Params) (hp : p.WellFormed) (msg : List ℕ) (hb : ∀ b ∈ msg, b < 256)
    (hlen : msg.length < 2 ^ 64) :
    hash p msg < 2 ^ 128 ∧ fe (hash p msg) = raijuhash ιGF (keyOf p) (msgOf msg) := by
  have hL : (msgOf msg).length = msg.length := List.length_map _
  have ha := hp.2.1
  have hbk := hp.2.2.1
  have ht := hp.2.2.2.2.2.1
  have hs := hp.2.2.2.2.2.2.1
  by_cases h16 : msg.length < 16
  · have hmod : msg.length % 256 = msg.length := Nat.mod_eq_of_lt (by omega)
    obtain ⟨hx, ex⟩ := toPoly_tagged msg hb (by omega) msg.length (by omega)
    have hh : hash p msg = gfMul (fromLE ((pad16 msg).set 15 msg.length)) p.a ^^^ p.s := by
      unfold hash
      simp only [h16, ↓reduceIte, hmod]
    rw [hh]
    refine ⟨Nat.xor_lt_two_pow (gfMul_spec hx ha).1 hs, ?_⟩
    rw [fe_xor, fe_gfMul hx ha]
    simp only [raijuhash, hL, h16, ↓reduceIte, hashShort1, shortX0, keyOf]
    rw [fe, ex]
  · by_cases h32 : msg.length < 32
    · have hmod : msg.length % 256 = msg.length := Nat.mod_eq_of_lt (by omega)
      have hdb : ∀ b ∈ msg.drop 16, b < 256 := fun b h => hb b (List.mem_of_mem_drop h)
      obtain ⟨hx0, ex0⟩ := toPoly_first16 msg hb
      obtain ⟨hx1, ex1⟩ := toPoly_tagged (msg.drop 16) hdb (by simp; omega) msg.length
        (by omega)
      have hh : hash p msg = gfMul (fromLE (msg.take 16)) p.a ^^^
          gfMul (fromLE ((pad16 (msg.drop 16)).set 15 msg.length)) p.b ^^^ p.s := by
        unfold hash
        simp only [h16, h32, ↓reduceIte, hmod]
      rw [hh]
      refine ⟨Nat.xor_lt_two_pow (Nat.xor_lt_two_pow (gfMul_spec hx0 ha).1
        (gfMul_spec hx1 hbk).1) hs, ?_⟩
      rw [fe_xor, fe_xor, fe_gfMul hx0 ha, fe_gfMul hx1 hbk]
      simp only [raijuhash, hL, h16, h32, ↓reduceIte, hashShort2, keyOf]
      unfold fe
      rw [ex0, ex1]
      simp only [msgOf, List.map_drop]
    · have hq : ∀ i ∈ List.range (numChunks (msgOf msg)),
          (chunkPair p (slice msg i)).1 < 2 ^ 128 ∧ (chunkPair p (slice msg i)).2 < 2 ^ 128 ∧
          (fe (chunkPair p (slice msg i)).1, fe (chunkPair p (slice msg i)).2) =
            FullProof.digAt ιGF (msgOf msg) (keyTable p) i := by
        intro i hi
        have hi' : i < numChunks (msgOf msg) := List.mem_range.mp hi
        obtain ⟨h1, h2, e⟩ := chunkPair_spec p hp (chunkRel_msg msg hb i hi')
        refine ⟨h1, h2, ?_⟩
        rw [e, FullProof.digAt]
        simp only [hi', dite_true]
      obtain ⟨hacc, eacc⟩ := outer_fold p hp msg _ _ hq 0 (by positivity)
      have hmod : msg.length % 2 ^ 64 = msg.length := Nat.mod_eq_of_lt hlen
      have hlt : msg.length < 2 ^ 128 := hlen.trans (by norm_num)
      have hh : hash p msg = ((List.range (numChunks (msgOf msg))).map (slice msg)).foldl
          (outerStep p) 0 ^^^ gfMul msg.length p.t ^^^ p.s := by
        rw [← chunks_eq]
        unfold hash
        simp only [h16, h32, ↓reduceIte, hmod]
        rfl
      rw [hh]
      refine ⟨Nat.xor_lt_two_pow (Nat.xor_lt_two_pow hacc (gfMul_spec hlt ht).1) hs, ?_⟩
      rw [fe_xor, fe_xor, fe_gfMul hlt ht, eacc, fe_zero]
      simp only [raijuhash, hL, h16, h32, ↓reduceIte, hashLong, keyOf]
      rw [FullProof.digests_eq, accumR, lenPoly, bitPoly_eq_toPoly hlen]
      rfl

end RaijuHash.Reference

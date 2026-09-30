import RaijuHash.ReferenceProof.Arith

/-!
# `chunk_pair` computes `chunkDigest`

`ChunkRel cs c` says the Rust chunk slice `cs` and the model chunk `c` have
the same blocks. Under it, each position sum `p_j` of `chunk_pair` is the
model's `S[j]` as a polynomial (`pjOf_spec`), and the `(h0, h1)` pair is the
model digest in `GF128` (`chunkPair_spec`).
-/

namespace RaijuHash.Reference
open Polynomial RaijuHash

/-! ### Rust values as model values -/

/-- A `u8` as the model's eight bits. -/
def byteOf (n : ℕ) : Byte := fun i => if n.testBit i then 1 else 0

/-- A `u64` as a model word. -/
noncomputable def toWord (n : ℕ) : Word :=
  ⟨toPoly (n % 2 ^ 64), mem_degreeLT.mpr (toPoly_degree (Nat.mod_lt _ (by positivity)))⟩

lemma toWord_coe {n : ℕ} (h : n < 2 ^ 64) : (toWord n : R) = toPoly n := by
  change toPoly (n % 2 ^ 64) = toPoly n
  rw [Nat.mod_eq_of_lt h]

/-- A `u128` as a field element. -/
noncomputable def fe (n : ℕ) : GF128 := ιGF (toPoly n)

lemma fe_xor (a b : ℕ) : fe (a ^^^ b) = fe a + fe b := by
  simp [fe, toPoly_xor]

lemma fe_gfMul {a b : ℕ} (ha : a < 2 ^ 128) (hb : b < 2 ^ 128) :
    fe (gfMul a b) = fe a * fe b := (gfMul_spec ha hb).2

/-- The model key table of Rust parameters. -/
noncomputable def keyTable (p : Params) : KeyTable :=
  fun j l => (toWord (p.row j l), toWord (p.row j (8 + l)))

/-- The model key of Rust parameters. -/
noncomputable def keyOf (p : Params) : HashKey GF128 :=
  ⟨keyTable p, fe p.a, fe p.b, fe p.r, fe p.r2, fe p.t, fe p.s⟩

lemma row_lt {p : Params} (hp : p.WellFormed) (j i : ℕ) : p.row j i < 2 ^ 64 := by
  unfold Params.row
  split_ifs
  · exact hp.1 _ _
  · positivity

/-! ### Folds of XOR -/

lemma xor_fold_lt (f : ℕ → ℕ) (hf : ∀ i, f i < 2 ^ 128) (l : List ℕ) (a : ℕ) (ha : a < 2 ^ 128) :
    l.foldl (fun acc i => acc ^^^ f i) a < 2 ^ 128 := by
  induction l generalizing a with
  | nil => exact ha
  | cons i l ih => exact ih _ (Nat.xor_lt_two_pow ha (hf i))

lemma toPoly_xor_fold (f : ℕ → ℕ) (n : ℕ) :
    toPoly ((List.range n).foldl (fun acc i => acc ^^^ f i) 0) =
      ∑ i ∈ Finset.range n, toPoly (f i) := by
  induction n with
  | zero => simp
  | succ n ih =>
    rw [List.range_succ, List.foldl_append, List.foldl_cons, List.foldl_nil, toPoly_xor, ih,
      Finset.sum_range_succ]

/-! ### Words -/

lemma word_lt (bs : List ℕ) (hb : ∀ b ∈ bs, b < 256) (i : ℕ) : word bs i < 2 ^ 64 := by
  unfold word
  have hm : ∀ b ∈ (bs.drop (8 * i)).take 8, b < 256 :=
    fun b h => hb b (List.mem_of_mem_drop (List.mem_of_mem_take h))
  exact (fromLE_lt _ hm).trans_le (Nat.pow_le_pow_right (by norm_num) (by simp; omega))

/-! ### `chunk_pair` -/

section Chunk
variable (p : Params) (cs : List ℕ)

def nbOf : ℕ := (cs.length + 127) / 128

def blockOf (b : ℕ) : List ℕ := (List.range 128).map fun k => cs.getD (128 * b + k) 0

def wOf (b l : ℕ) : ℕ := if b < nbOf cs then word (blockOf cs b) l else 0

/-- `p_j`: the position sum at encoded position `j`. -/
def pjOf (j : ℕ) : ℕ := (List.range 8).foldl (fun pj l =>
  let prev : ℕ → ℕ := fun i => if j > 0 then wOf cs (j - 1) i else 0
  let tx := prev l ^^^ wOf cs j l ^^^ p.row j l
  let ty := prev (8 + l) ^^^ wOf cs j (8 + l) ^^^ p.row j (8 + l)
  pj ^^^ clmul64 tx ty) 0

def txOf (j l : ℕ) : ℕ :=
  (if j > 0 then wOf cs (j - 1) l else 0) ^^^ wOf cs j l ^^^ p.row j l

def tyOf (j l : ℕ) : ℕ :=
  (if j > 0 then wOf cs (j - 1) (8 + l) else 0) ^^^ wOf cs j (8 + l) ^^^ p.row j (8 + l)

lemma pjOf_eq (j : ℕ) :
    pjOf p cs j = (List.range 8).foldl (fun acc l => acc ^^^ clmul64 (txOf p cs j l) (tyOf p cs j l)) 0 :=
  rfl

def chunkStep (h : ℕ × ℕ) (j : ℕ) : ℕ × ℕ :=
  if j = 64 then (h.1, h.2 ^^^ pjOf p cs j) else (h.1 ^^^ pjOf p cs j, h.2 ^^^ gfMul j (pjOf p cs j))

lemma chunkPair_eq : chunkPair p cs = (List.range (nbOf cs + 1)).foldl (chunkStep p cs) (0, 0) :=
  rfl

end Chunk

lemma blockOf_bytes (cs : List ℕ) (hb : ∀ b ∈ cs, b < 256) (b : ℕ) :
    ∀ x ∈ blockOf cs b, x < 256 := by
  intro x hx
  simp only [blockOf, List.mem_map, List.mem_range] at hx
  obtain ⟨k, -, rfl⟩ := hx
  rw [List.getD_eq_getElem?_getD]
  cases h : cs[128 * b + k]? with
  | none => simp
  | some y => exact hb y (List.mem_of_getElem? h)

lemma wOf_lt (cs : List ℕ) (hb : ∀ b ∈ cs, b < 256) (b l : ℕ) : wOf cs b l < 2 ^ 64 := by
  unfold wOf
  split_ifs
  · exact word_lt _ (blockOf_bytes cs hb b) l
  · positivity

lemma toWord_xor (a b : ℕ) : toWord (a ^^^ b) = toWord a + toWord b := by
  apply Subtype.ext
  change toPoly ((a ^^^ b) % 2 ^ 64) = toPoly (a % 2 ^ 64) + toPoly (b % 2 ^ 64)
  rw [Nat.xor_mod_two_pow, toPoly_xor]

lemma toWord_zero : toWord 0 = 0 := by
  apply Subtype.ext
  change toPoly (0 % 2 ^ 64) = 0
  simp

lemma keyAt_keyTable (p : Params) {j : ℕ} (hj : j < 65) (l : Lane) :
    keyAt (keyTable p) j l = (toWord (p.row j l), toWord (p.row j (8 + l))) := by
  simp [keyAt, hj, keyTable]

/-- The hypotheses relating a model chunk to a Rust chunk slice. -/
structure ChunkRel (cs : List ℕ) (c : Chunk) : Prop where
  bytes : ∀ b ∈ cs, b < 256
  n_eq : c.n = nbOf cs
  blocks : ∀ b < c.n, ∀ l : Lane, c.blocks b l = (toWord (wOf cs b l), toWord (wOf cs b (8 + l)))

lemma encodedPair_ref (p : Params) {cs : List ℕ} {c : Chunk} (hc : ChunkRel cs c)
    {j : ℕ} (hj : j ≤ c.n) (l : Lane) :
    encodedPair c (keyTable p) j l = (toWord (txOf p cs j l), toWord (tyOf p cs j l)) := by
  have hj65 : j < 65 := by have := c.hn_le; omega
  have hprev : (if j = 0 then ((0 : Word), (0 : Word)) else c.blocks (j - 1) l) =
      (toWord (if j > 0 then wOf cs (j - 1) l else 0),
        toWord (if j > 0 then wOf cs (j - 1) (8 + l) else 0)) := by
    by_cases h0 : j = 0
    · simp [h0, toWord_zero]
    · simp only [h0, ↓reduceIte]
      rw [hc.blocks (j - 1) (by omega) l]
      simp [show j > 0 by omega]
  have hcur : (if j = c.n then ((0 : Word), (0 : Word)) else c.blocks j l) =
      (toWord (wOf cs j l), toWord (wOf cs j (8 + l))) := by
    by_cases hn : j = c.n
    · have : ¬ c.n < nbOf cs := by rw [hc.n_eq]; omega
      simp [hn, wOf, this, toWord_zero]
    · simp only [hn, ↓reduceIte]
      rw [hc.blocks j (by omega) l]
  simp only [encodedPair, hprev, hcur, keyAt_keyTable p hj65, encode, txOf, tyOf, toWord_xor]

lemma pjOf_spec (p : Params) (hp : p.WellFormed) {cs : List ℕ} {c : Chunk} (hc : ChunkRel cs c)
    {j : ℕ} (hj : j ≤ c.n) :
    pjOf p cs j < 2 ^ 128 ∧ toPoly (pjOf p cs j) = chunkS c (keyTable p) j := by
  have htx : ∀ l, txOf p cs j l < 2 ^ 64 := fun l => by
    unfold txOf
    refine Nat.xor_lt_two_pow (Nat.xor_lt_two_pow ?_ (wOf_lt cs hc.bytes _ _)) (row_lt hp _ _)
    split_ifs
    · exact wOf_lt cs hc.bytes _ _
    · positivity
  have hty : ∀ l, tyOf p cs j l < 2 ^ 64 := fun l => by
    unfold tyOf
    refine Nat.xor_lt_two_pow (Nat.xor_lt_two_pow ?_ (wOf_lt cs hc.bytes _ _)) (row_lt hp _ _)
    split_ifs
    · exact wOf_lt cs hc.bytes _ _
    · positivity
  rw [pjOf_eq]
  refine ⟨xor_fold_lt _ (fun l => clmul64_lt _ _) _ 0 (by positivity), ?_⟩
  rw [toPoly_xor_fold, chunkS, Finset.sum_range]
  refine Finset.sum_congr rfl fun l _ => ?_
  rw [encodedPair_ref p hc hj l, clmul, toWord_coe (htx l), toWord_coe (hty l),
    toPoly_clmul64 (htx l) (hty l)]

lemma chunk_fold (p : Params) (cs : List ℕ) (n : ℕ) (hn : n ≤ 65)
    (hpj : ∀ j < n, pjOf p cs j < 2 ^ 128) :
    ((List.range n).foldl (chunkStep p cs) (0, 0)).1 < 2 ^ 128 ∧
    ((List.range n).foldl (chunkStep p cs) (0, 0)).2 < 2 ^ 128 ∧
    fe ((List.range n).foldl (chunkStep p cs) (0, 0)).1 =
      ∑ j ∈ Finset.range n, (if j = 64 then 0 else fe (pjOf p cs j)) ∧
    fe ((List.range n).foldl (chunkStep p cs) (0, 0)).2 =
      ∑ j ∈ Finset.range n, (if j = 64 then fe (pjOf p cs j) else fe j * fe (pjOf p cs j)) := by
  induction n with
  | zero => simp [fe]
  | succ n ih =>
    obtain ⟨h1, h2, e1, e2⟩ := ih (by omega) fun j hj => hpj j (by omega)
    have hp := hpj n (by omega)
    rw [List.range_succ, List.foldl_append, List.foldl_cons, List.foldl_nil,
      Finset.sum_range_succ, Finset.sum_range_succ]
    generalize (List.range n).foldl (chunkStep p cs) (0, 0) = h at h1 h2 e1 e2
    unfold chunkStep
    by_cases h64 : n = 64
    · subst h64
      simp only [↓reduceIte]
      refine ⟨h1, Nat.xor_lt_two_pow h2 hp, by rw [e1, add_zero], by rw [fe_xor, e2]⟩
    · have hn128 : n < 2 ^ 128 := by omega
      simp only [h64, ↓reduceIte]
      refine ⟨Nat.xor_lt_two_pow h1 hp, Nat.xor_lt_two_pow h2 (gfMul_spec hn128 hp).1,
        by rw [fe_xor, e1], by rw [fe_xor, e2, fe_gfMul hn128 hp]⟩

/-- Positions `0..=N` with the special column at 64, split as in `chunkDigest`. -/
lemma sum_skip64 {M : Type} [AddCommMonoid M] (N : ℕ) (hN : N ≤ 64) (f g : ℕ → M) :
    ∑ j ∈ Finset.range (N + 1), (if j = 64 then g j else f j) =
      ∑ j ∈ Finset.range (min N 63 + 1), f j + (if N = 64 then g 64 else 0) := by
  by_cases h64 : N = 64
  · subst h64
    have hlow : ∑ j ∈ Finset.range 64, (if j = 64 then g j else f j) =
        ∑ j ∈ Finset.range 64, f j := Finset.sum_congr rfl fun j hj => by
      rw [Finset.mem_range] at hj
      simp only [show j ≠ 64 by omega, ↓reduceIte]
    rw [Finset.sum_range_succ, show min 64 63 + 1 = 64 by decide, hlow]
    simp only [↓reduceIte]
  · rw [show min N 63 + 1 = N + 1 by omega]
    simp only [h64, ↓reduceIte, add_zero]
    refine Finset.sum_congr rfl fun j hj => ?_
    rw [Finset.mem_range] at hj
    simp only [show j ≠ 64 by omega, ↓reduceIte]

/-- **`chunk_pair` computes the chunk digest.** -/
theorem chunkPair_spec (p : Params) (hp : p.WellFormed) {cs : List ℕ} {c : Chunk}
    (hc : ChunkRel cs c) :
    (chunkPair p cs).1 < 2 ^ 128 ∧ (chunkPair p cs).2 < 2 ^ 128 ∧
      (fe (chunkPair p cs).1, fe (chunkPair p cs).2) = chunkDigest ιGF c (keyTable p) := by
  have hcn := c.hn_le
  have hpj : ∀ j < nbOf cs + 1, pjOf p cs j < 2 ^ 128 :=
    fun j hj => (pjOf_spec p hp hc (by rw [hc.n_eq]; omega)).1
  have hS : ∀ j < c.n + 1, fe (pjOf p cs j) = ιGF (chunkS c (keyTable p) j) :=
    fun j hj => by rw [fe, (pjOf_spec p hp hc (by omega)).2]
  obtain ⟨h1, h2, e1, e2⟩ := chunk_fold p cs (nbOf cs + 1) (by rw [← hc.n_eq]; omega) hpj
  rw [chunkPair_eq]
  refine ⟨h1, h2, ?_⟩
  rw [e1, e2, ← hc.n_eq, sum_skip64 _ hcn, sum_skip64 _ hcn, ite_self, add_zero]
  simp only [chunkDigest]
  refine Prod.ext ?_ ?_ <;> dsimp only
  · exact Finset.sum_congr rfl fun j hj => hS j (by rw [Finset.mem_range] at hj; omega)
  · congr 1
    · refine Finset.sum_congr rfl fun j hj => ?_
      rw [Finset.mem_range] at hj
      rw [hS j (by omega), fe, bitPoly_eq_toPoly (by omega : j < 2 ^ 6)]
    · by_cases h64 : c.n = 64
      · simp only [h64, ↓reduceIte]
        exact hS 64 (by omega)
      · simp only [h64, ↓reduceIte]

end RaijuHash.Reference

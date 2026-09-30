import RaijuHash.FullProof.Basic
import RaijuHash.ChunkProof.Main

/-!
# Theorem 5, case 3: both messages long, equal length

* `exists_chunk_diff`: distinct equal-length messages have a chunk index at
  which the chunks have the same number of blocks and differ in a block.
* `long_hash_diff`: the output difference is the formal outer polynomial of
  the per-chunk digest differences, evaluated at `(R, R2)`; the length and
  offset terms cancel.
* `outerPoly_eq_C`: that polynomial is a constant only if every chunk
  difference and the constant are zero (its monomials are non-constant and
  distinct per chunk coordinate).
* `card_both_long`: condition on the table. A nonzero event polynomial has
  total degree at most `q` and at most `q · |F|` zeros (Schwartz–Zippel).
  An identically zero one forces the differing chunk to collide, which by
  Proposition 4 happens for at most `2⁻¹²⁸` of the tables.
-/

namespace RaijuHash.FullProof
open Polynomial

/-! ### Messages to chunks -/

lemma byteAt_eq_of_wordAt_eq {m m' : List Byte} {w : ℕ} (h : wordAt m w = wordAt m' w)
    (p : ℕ) (hp : p / 8 = w) : byteAt m p = byteAt m' p := by
  have hf := congrArg (degreeLTEquiv (ZMod 2) 64) h
  simp only [wordAt, LinearEquiv.apply_symm_apply] at hf
  funext bit
  have hb := bit.isLt
  have := congrFun hf ⟨8 * (p % 8) + bit.val, by omega⟩
  have hk : 8 * w + (8 * (p % 8) + bit.val) / 8 = p := by omega
  have hk' : (8 * (p % 8) + bit.val) % 8 = bit.val := by omega
  simp only [hk, hk'] at this
  exact this

lemma msgChunk_n (m : List Byte) (i : Fin (numChunks m)) :
    (msgChunk m i).n = min 64 (numBlocks m - 64 * i.val) := rfl

/-- Distinct equal-length messages have a chunk index where the two chunks
    have equal lengths and differ in some block. -/
lemma exists_chunk_diff {m m' : List Byte} (hlen : m.length = m'.length) (hm : m ≠ m') :
    ∃ i, ∃ (h : i < numChunks m) (h' : i < numChunks m'),
      (msgChunk m ⟨i, h⟩).n = (msgChunk m' ⟨i, h'⟩).n ∧
      ∃ b, b < (msgChunk m ⟨i, h⟩).n ∧
        (msgChunk m ⟨i, h⟩).blocks b ≠ (msgChunk m' ⟨i, h'⟩).blocks b := by
  obtain ⟨p, hp, hne⟩ : ∃ p, p < m.length ∧ byteAt m p ≠ byteAt m' p := by
    by_contra hall
    push Not at hall
    apply hm
    apply List.ext_getElem hlen
    intro p h₁ h₂
    have := hall p h₁
    simpa only [byteAt, List.getD_eq_getElem _ _ h₁, List.getD_eq_getElem _ _ h₂] using this
  have hblocks : numBlocks m = numBlocks m' := by simp only [numBlocks, hlen]
  have hi : p / 8192 < numChunks m := by simp only [numChunks, numBlocks]; omega
  have hi' : p / 8192 < numChunks m' := by simp only [numChunks, numBlocks, ← hlen]; omega
  refine ⟨p / 8192, hi, hi', ?_, p % 8192 / 128, ?_, ?_⟩
  · simp only [msgChunk_n, hblocks]
  · simp only [msgChunk_n, numBlocks]
    omega
  · intro hb
    apply hne
    have hw : p / 8 = 16 * (64 * (p / 8192) + p % 8192 / 128) + p % 128 / 8 := by omega
    by_cases ht : p % 128 / 8 < 8
    · have := congrArg Prod.fst (congrFun hb ⟨p % 128 / 8, ht⟩)
      exact byteAt_eq_of_wordAt_eq this p hw
    · have := congrArg Prod.snd (congrFun hb ⟨p % 128 / 8 - 8, by omega⟩)
      exact byteAt_eq_of_wordAt_eq this p (by simp only; omega)

/-! ### The outer polynomial -/

section Outer
variable {F : Type} [Field F]

lemma outerStep_add (p p' : OuterPoly F) (h h' : F × F) :
    outerStep p h + outerStep p' h' = outerStep (p + p') (h + h') := by
  simp only [outerStep, Prod.fst_add, Prod.snd_add, map_add]
  ring

lemma outerFold_map_add {α : Type} (xs : List α) (f g : α → F × F) (p p' : OuterPoly F) :
    (xs.map f).foldl outerStep p + (xs.map g).foldl outerStep p' =
      (xs.map fun a => f a + g a).foldl outerStep (p + p') := by
  induction xs generalizing p p' with
  | nil => rfl
  | cons a xs ih =>
    simp only [List.map_cons, List.foldl_cons]
    rw [ih, outerStep_add]

lemma outerPoly_map_add {α : Type} (xs : List α) (f g : α → F × F) :
    outerPoly (xs.map f) + outerPoly (xs.map g) = outerPoly (xs.map fun a => f a + g a) := by
  simpa only [outerPoly, add_zero] using outerFold_map_add xs f g 0 0

lemma outerPoly_append_singleton (ds : List (F × F)) (δ : F × F) :
    outerPoly (ds ++ [δ]) = outerStep (outerPoly ds) δ := by
  simp only [outerPoly, List.foldl_append, List.foldl_cons, List.foldl_nil]

/-- Every monomial of the outer polynomial is non-constant and belongs to a
    single chunk coordinate, so it equals a constant only when all chunk
    coordinates and the constant are zero. -/
lemma outerPoly_eq_C (ds : List (F × F)) (e : F) (h : outerPoly ds = MvPolynomial.C e) :
    e = 0 ∧ ∀ δ ∈ ds, δ = 0 := by
  induction ds using List.reverseRecOn generalizing e with
  | nil =>
    refine ⟨?_, by simp⟩
    have h0 : (0 : OuterPoly F) = MvPolynomial.C e := h
    exact (MvPolynomial.C_eq_zero.mp h0.symm)
  | append_singleton ds δ ih =>
    rw [outerPoly_append_singleton] at h
    have h00 := congrArg (MvPolynomial.eval ![0, 0]) h
    have h01 := congrArg (MvPolynomial.eval ![0, 1]) h
    simp only [eval_outerStep, MvPolynomial.eval_C, Matrix.cons_val_zero,
      Matrix.cons_val_one, mul_zero, mul_one, zero_add] at h00 h01
    have he : e = 0 := h00.symm
    have hδ₂ : δ.2 = 0 := h01.trans he
    have hmul : (outerPoly ds + MvPolynomial.C δ.1) * MvPolynomial.X 0 = 0 := by
      have := h
      simp only [outerStep, hδ₂, map_zero, zero_mul, add_zero, he] at this
      exact this
    have hsum := (mul_eq_zero.mp hmul).resolve_right (MvPolynomial.X_ne_zero 0)
    have hP : outerPoly ds = MvPolynomial.C (-δ.1) := by
      rw [map_neg]
      exact eq_neg_of_add_eq_zero_left hsum
    obtain ⟨hδ₁, hall⟩ := ih (-δ.1) hP
    refine ⟨he, fun x hx => ?_⟩
    rcases List.mem_append.mp hx with hx | hx
    · exact hall x hx
    · rw [List.mem_singleton.mp hx]
      exact Prod.ext (neg_eq_zero.mp hδ₁) hδ₂

end Outer

/-! ### Both messages long, equal length -/

section Long
variable {F : Type} [Field F] [CharP F 2]

/-- The chunk digest at a natural-number index, zero past the last chunk. -/
noncomputable def digAt (ι : R →+ F) (m : List Byte) (K : KeyTable) (i : ℕ) : F × F :=
  if h : i < numChunks m then chunkDigest ι (msgChunk m ⟨i, h⟩) K else 0

lemma digests_eq (ι : R →+ F) (m : List Byte) (K : KeyTable) :
    (List.finRange (numChunks m)).map (fun i => chunkDigest ι (msgChunk m i) K) =
      (List.range (numChunks m)).map (digAt ι m K) := by
  apply List.ext_getElem (by simp)
  intro i h₁ h₂
  have hi : i < numChunks m := by simpa using h₂
  simp only [List.getElem_map, List.getElem_finRange, List.getElem_range, digAt, hi, dite_true]
  rfl

/-- The per-chunk digest differences under one table. -/
noncomputable def diffs (ι : R →+ F) (m m' : List Byte) (K : KeyTable) : List (F × F) :=
  (List.range (numChunks m)).map fun i => digAt ι m K i + digAt ι m' K i

/-- For long messages of equal length the length and offset terms cancel and
    the output difference is the outer polynomial of the chunk differences. -/
lemma long_hash_diff (ι : R →+ F) (m m' : List Byte) (hlen : m.length = m'.length)
    (hlong : 32 ≤ m.length) (k : HashKey F) :
    raijuhash ι k m + raijuhash ι k m' =
      MvPolynomial.eval ![k.r, k.r2] (outerPoly (diffs ι m m' k.table)) := by
  have hq : numChunks m' = numChunks m := by simp only [numChunks, numBlocks, hlen]
  have h₁ : ¬ m.length < 16 := by omega
  have h₂ : ¬ m.length < 32 := by omega
  have h₁' : ¬ m'.length < 16 := by omega
  have h₂' : ¬ m'.length < 32 := by omega
  simp only [raijuhash, h₁, h₂, h₁', h₂', ite_false, hashLong]
  rw [digests_eq, digests_eq, hq, diffs, ← outerPoly_map_add, map_add, eval_outerPoly,
    eval_outerPoly, ← hlen]
  have htwo : (2 : F) = 0 := CharTwo.two_eq_zero
  linear_combination (ι (lenPoly m.length) * k.t + k.s) * htwo

/-- The collision event for a fixed table, as a polynomial in `(R, R2)`. -/
noncomputable def eventPoly (ι : R →+ F) (m m' : List Byte) (d : F) (K : KeyTable) :
    OuterPoly F :=
  outerPoly (diffs ι m m' K) - MvPolynomial.C d

lemma eventPoly_degree (ι : R →+ F) (m m' : List Byte) (d : F) (K : KeyTable) :
    (eventPoly ι m m' d K).totalDegree ≤ numChunks m :=
  (MvPolynomial.totalDegree_sub_C_le _ _).trans
    ((degree_outerPoly _).trans (by simp [diffs]))

/-- If the event polynomial vanishes identically, every chunk pair collides. -/
lemma eventPoly_zero (ι : R →+ F) (m m' : List Byte) (d : F) (K : KeyTable)
    (h : eventPoly ι m m' d K = 0) (i : ℕ) (hi : i < numChunks m) :
    digAt ι m K i + digAt ι m' K i = 0 :=
  (outerPoly_eq_C _ d (sub_eq_zero.mp h)).2 _
    (List.mem_map.mpr ⟨i, List.mem_range.mpr hi, rfl⟩)

omit [CharP F 2] in
/-- Separate the table and `(R, R2)` from the keys the long event ignores. -/
def hashKeyOuterEquiv (F : Type) :
    HashKey F ≃ KeyTable × ((F × F × F × F) × (Fin 2 → F)) where
  toFun k := (k.table, ((k.a, k.b, k.t, k.s), ![k.r, k.r2]))
  invFun z := ⟨z.1, z.2.1.1, z.2.1.2.1, z.2.2 0, z.2.2 1, z.2.1.2.2.1, z.2.1.2.2.2⟩
  left_inv _ := rfl
  right_inv z := by
    obtain ⟨K, ⟨a, b, t, s⟩, v⟩ := z
    refine Prod.ext rfl (Prod.ext rfl ?_)
    funext i
    fin_cases i <;> rfl

omit [CharP F 2] in
/-- Schwartz–Zippel as a count over both outer keys. -/
lemma card_zeros_le [Fintype F] (p : OuterPoly F) (hp : p ≠ 0) :
    Nat.card {v : Fin 2 → F // MvPolynomial.eval v p = 0} ≤
      p.totalDegree * Fintype.card F := by
  classical
  have h := MvPolynomial.schwartz_zippel_totalDegree hp (Finset.univ : Finset F)
  simp only [Fintype.piFinset_univ, Finset.card_univ] at h
  have hF : (0 : ℚ≥0) < Fintype.card F := by exact_mod_cast Fintype.card_pos
  rw [div_le_div_iff₀ (by positivity) hF, pow_two, ← mul_assoc] at h
  rw [Nat.card_eq_fintype_card, Fintype.card_subtype]
  exact_mod_cast le_of_mul_le_mul_right h hF

lemma card_subtype_prod_eq_sum {A B : Type} [Fintype A] [Finite B] (P : A × B → Prop) :
    Nat.card {z // P z} = ∑ a, Nat.card {b // P (a, b)} := by
  rw [Nat.card_congr (Equiv.subtypeProdEquivSigmaSubtype (fun a b => P (a, b))),
    Nat.card_sigma]

lemma sum_ite_card {A : Type} [Fintype A] (P : A → Prop) [DecidablePred P] (c : ℕ) :
    ∑ a, (if P a then c else 0) = Nat.card {a // P a} * c := by
  rw [Finset.sum_ite, Finset.sum_const, Finset.sum_const_zero, smul_eq_mul, add_zero,
    Nat.card_eq_fintype_card, Fintype.card_subtype]

/-- **Both long, equal length.** Condition on the table: a nonzero event
    polynomial has at most `q·|F|` zeros in `(R, R2)` (Schwartz–Zippel); an
    identically zero one forces a collision of the first differing chunk,
    which happens for at most `2⁻¹²⁸` of the tables (Proposition 4). -/
lemma card_both_long [Fintype F] (hF : Fintype.card F = 2 ^ 128) (ι : R →+ F)
    (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (m m' : List Byte) (hm : m ≠ m') (hlen : m.length = m'.length)
    (hlong : 32 ≤ m.length) (d : F) :
    Nat.card {k : HashKey F // raijuhash ι k m + raijuhash ι k m' = d} * 2 ^ 128 ≤
      (numChunks m + 1) * Nat.card (HashKey F) := by
  classical
  have := Fintype.ofFinite KeyTable
  let e := hashKeyOuterEquiv F
  let E : HashKey F → Prop := fun k => raijuhash ι k m + raijuhash ι k m' = d
  let bad : KeyTable → Prop := fun K => eventPoly ι m m' d K = 0
  have hcardF : Nat.card F = 2 ^ 128 := Nat.card_eq_fintype_card.trans hF
  have hV : Nat.card (Fin 2 → F) = 2 ^ 256 := by
    rw [Nat.card_fun, hcardF, Nat.card_eq_fintype_card, Fintype.card_fin]
    norm_num
  -- The event only depends on the table and `(R, R2)`.
  have hevent (K : KeyTable) (w : (F × F × F × F) × (Fin 2 → F)) :
      E (e.symm (K, w)) ↔ MvPolynomial.eval w.2 (eventPoly ι m m' d K) = 0 := by
    have hv : ![(e.symm (K, w)).r, (e.symm (K, w)).r2] = w.2 := by
      funext i
      fin_cases i <;> rfl
    simp only [E]
    rw [long_hash_diff ι m m' hlen hlong, hv, eventPoly, map_sub, MvPolynomial.eval_C,
      sub_eq_zero]
    rfl
  -- Per-table bound.
  have hterm (K : KeyTable) :
      Nat.card {w : (F × F × F × F) × (Fin 2 → F) //
        MvPolynomial.eval w.2 (eventPoly ι m m' d K) = 0} ≤
      Nat.card (F × F × F × F) * (numChunks m * 2 ^ 128 + if bad K then 2 ^ 256 else 0) := by
    apply ChunkProof.card_subtype_prod_le
      (fun w : (F × F × F × F) × (Fin 2 → F) =>
        MvPolynomial.eval w.2 (eventPoly ι m m' d K) = 0)
    intro _
    by_cases hb : bad K
    · simp only [hb, ↓reduceIte]
      calc _ ≤ Nat.card (Fin 2 → F) := Finite.card_subtype_le _
        _ = 2 ^ 256 := hV
        _ ≤ _ := Nat.le_add_left _ _
    · simp only [hb, ↓reduceIte, add_zero]
      calc _ ≤ (eventPoly ι m m' d K).totalDegree * Fintype.card F := card_zeros_le _ hb
        _ ≤ numChunks m * 2 ^ 128 := by
          rw [hF]
          exact Nat.mul_le_mul_right _ (eventPoly_degree ι m m' d K)
  -- Tables whose event polynomial vanishes are rare.
  have hbad : Nat.card {K // bad K} * 2 ^ 128 ≤ Nat.card KeyTable := by
    obtain ⟨i, hi, hi', hn, hdiff⟩ := exists_chunk_diff hlen hm
    have hsub (K : KeyTable) (hK : bad K) :
        chunkDigest ι (msgChunk m ⟨i, hi⟩) K + chunkDigest ι (msgChunk m' ⟨i, hi'⟩) K = 0 := by
      have := eventPoly_zero ι m m' d K hK i hi
      simpa only [digAt, hi, hi', dite_true] using this
    calc Nat.card {K // bad K} * 2 ^ 128
        ≤ Nat.card {K : KeyTable // chunkDigest ι (msgChunk m ⟨i, hi⟩) K +
            chunkDigest ι (msgChunk m' ⟨i, hi'⟩) K = 0} * 2 ^ 128 := by
          apply Nat.mul_le_mul_right
          exact Nat.card_le_card_of_injective (fun K => ⟨K.val, hsub K.val K.property⟩)
            (fun a b h => by
              simp only [Subtype.mk.injEq] at h
              exact Subtype.ext h)
      _ ≤ Nat.card KeyTable := ChunkProof.card_chunk_le ι hι _ _ hn hdiff 0
  -- Sum over tables.
  have hcount : Nat.card {k // E k} ≤ Nat.card (F × F × F × F) *
      (Nat.card KeyTable * (numChunks m * 2 ^ 128) + Nat.card {K // bad K} * 2 ^ 256) := by
    rw [Nat.card_congr (e.subtypeEquiv (p := E) (q := fun z => E (e.symm z))
      (fun k => by simp only [Equiv.symm_apply_apply])), card_subtype_prod_eq_sum]
    calc ∑ K, Nat.card {w // E (e.symm (K, w))}
        = ∑ K, Nat.card {w : (F × F × F × F) × (Fin 2 → F) //
            MvPolynomial.eval w.2 (eventPoly ι m m' d K) = 0} :=
          Finset.sum_congr rfl fun K _ => Nat.card_congr (Equiv.subtypeEquivRight (hevent K))
      _ ≤ ∑ K, Nat.card (F × F × F × F) *
            (numChunks m * 2 ^ 128 + if bad K then 2 ^ 256 else 0) :=
          Finset.sum_le_sum fun K _ => hterm K
      _ = _ := by
          rw [← Finset.mul_sum, Finset.sum_add_distrib, sum_ite_card, Finset.sum_const,
            Finset.card_univ, smul_eq_mul, ← Nat.card_eq_fintype_card]
  have hkey : Nat.card (HashKey F) =
      Nat.card KeyTable * (Nat.card (F × F × F × F) * 2 ^ 256) := by
    rw [Nat.card_congr e, Nat.card_prod, Nat.card_prod, hV]
  rw [hkey]
  calc Nat.card {k // E k} * 2 ^ 128
      ≤ Nat.card (F × F × F × F) * (Nat.card KeyTable * (numChunks m * 2 ^ 128) +
          Nat.card {K // bad K} * 2 ^ 256) * 2 ^ 128 := Nat.mul_le_mul_right _ hcount
    _ = Nat.card (F × F × F × F) * Nat.card KeyTable * numChunks m * 2 ^ 256 +
          Nat.card (F × F × F × F) * 2 ^ 256 * (Nat.card {K // bad K} * 2 ^ 128) := by ring
    _ ≤ Nat.card (F × F × F × F) * Nat.card KeyTable * numChunks m * 2 ^ 256 +
          Nat.card (F × F × F × F) * 2 ^ 256 * Nat.card KeyTable :=
        Nat.add_le_add_left (Nat.mul_le_mul_left _ hbad) _
    _ = _ := by ring

end Long

end RaijuHash.FullProof

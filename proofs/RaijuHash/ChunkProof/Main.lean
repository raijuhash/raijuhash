import RaijuHash.ChunkProof.Basic
import RaijuHash.NHProof.Main
import RaijuHash.ChainProof.Main

/-!
# Proof of Proposition 4: single chunk is `2⁻¹²⁸`-AXU

For distinct equal-length chunks and any target `(d₀, d₁)`, the chunk digests
collide at the target with probability `≤ 2⁻¹²⁸`, over uniform independent key
rows.

## Outline (SPEC.md Proposition 4)

`digest_diff_eq` writes the digest difference as a sum, over every position
`j ≤ 64` and lane `l`, of `column(j) · NH_{j,l}`, where `NH_{j,l}` is the
carryless NH difference and depends only on the table entry `K[j][l]`.
Unused positions get the zero column.

Lemma 2 gives a lane `l` and two positions `j₁ < j₂` whose key-free encoded
pairs differ. `splitEquiv` splits the whole table into those two entries and
the rest (with the two entries zeroed). For a fixed rest, the difference is a
fixed offset plus `col(j₁) · D₁ + col(j₂) · D₂` (`digestDiff_split`).

**Lemma 3**: columns `(1, j₁)` and `(1, j₂)` with `j₁ ≠ j₂` (both `< 64`) have
determinant `j₂ - j₁ ≠ 0`; `(1, j)` and `(0, 1)` have determinant `1`. So the
system determines `(D₁, D₂)` uniquely, and the independent two-NH count
(`two_nh_count`, from Lemma 1) bounds each fiber by `2¹²⁸` of `2²⁵⁶` values.
Summing over the rest gives the bound for the whole table.
-/

namespace RaijuHash.ChunkProof

open Polynomial

variable {F : Type} [Field F]

/-- The key-free part of an encoded pair. -/
noncomputable def dataPair (c : Chunk) (j : ℕ) (l : Lane) : Word × Word :=
  let prev : Word × Word := if j = 0 then (0, 0) else (c.blocks (j - 1) l)
  let cur : Word × Word := if j = c.n then (0, 0) else (c.blocks j l)
  (prev.1 + cur.1, prev.2 + cur.2)

lemma encodedPair_eq (c : Chunk) (K : KeyTable) (j : ℕ) (l : Lane) :
    encodedPair c K j l =
      ((dataPair c j l).1 + (keyAt K j l).1, (dataPair c j l).2 + (keyAt K j l).2) := rfl

/-- The NH difference at one position and lane, as a function of its key entry. -/
noncomputable def laneDiff (ι : R →+ F) (c c' : Chunk) (j : ℕ) (l : Lane)
    (ab : Word × Word) : F :=
  nhDifference ι (dataPair c j l).1 (dataPair c j l).2
    (dataPair c' j l).1 (dataPair c' j l).2 ab

lemma posDiff_eq (ι : R →+ F) (c c' : Chunk) (K : KeyTable) (j : ℕ) :
    ι (chunkS c K j) + ι (chunkS c' K j) =
      ∑ l : Lane, laneDiff ι c c' j l (keyAt K j l) := by
  simp only [chunkS, map_sum, ← Finset.sum_add_distrib]
  rfl

/-- The column of position `j` in a chunk of `n` blocks; unused positions
    get the zero column. -/
noncomputable def column (ι : R →+ F) (n j : ℕ) : F × F :=
  if j < min n 63 + 1 then (1, ι (bitPoly 6 j))
  else if j = 64 ∧ n = 64 then (0, 1) else (0, 0)

def scale (w : F × F) (t : F) : F × F := (w.1 * t, w.2 * t)

lemma scale_sum {α : Type} (s : Finset α) (w : F × F) (t : α → F) :
    scale w (∑ a ∈ s, t a) = ∑ a ∈ s, scale w (t a) := by
  simp only [scale, Finset.mul_sum]
  exact (Prod.fst_sum (s := s) (f := fun a => scale w (t a)) ▸
    Prod.snd_sum (s := s) (f := fun a => scale w (t a)) ▸ rfl)

variable [CharP F 2]

/-- The digest difference is a sum of column-weighted NH differences, one
    for each position and lane, each depending only on its own key entry. -/
lemma digest_diff_eq (ι : R →+ F) (c c' : Chunk) (hn : c.n = c'.n) (K : KeyTable) :
    chunkDigest ι c K + chunkDigest ι c' K =
      ∑ j ∈ Finset.range 65, ∑ l : Lane,
        scale (column ι c.n j) (laneDiff ι c c' j l (keyAt K j l)) := by
  have hD (j : ℕ) : ∑ l : Lane, scale (column ι c.n j) (laneDiff ι c c' j l (keyAt K j l)) =
      scale (column ι c.n j) (ι (chunkS c K j) + ι (chunkS c' K j)) := by
    rw [posDiff_eq, scale_sum]
  simp only [hD]
  have hb : min c.n 63 + 1 ≤ 65 := by omega
  rw [← Finset.sum_range_add_sum_Ico _ hb]
  have hlow : ∀ j ∈ Finset.range (min c.n 63 + 1),
      scale (column ι c.n j) (ι (chunkS c K j) + ι (chunkS c' K j)) =
        (ι (chunkS c K j) + ι (chunkS c' K j),
          ι (bitPoly 6 j) * ι (chunkS c K j) + ι (bitPoly 6 j) * ι (chunkS c' K j)) := by
    intro j hj
    rw [Finset.mem_range] at hj
    simp only [scale, column, hj, ite_true, one_mul, mul_add]
  have hhigh : ∑ j ∈ Finset.Ico (min c.n 63 + 1) 65,
      scale (column ι c.n j) (ι (chunkS c K j) + ι (chunkS c' K j)) =
        (0, if c.n = 64 then ι (chunkS c K 64) + ι (chunkS c' K 64) else 0) := by
    by_cases h64 : c.n = 64
    · simp [scale, column, h64]
    · rw [Finset.sum_eq_zero]
      · simp [h64]; rfl
      · intro j hj
        rw [Finset.mem_Ico] at hj
        simp [scale, column, h64, show ¬ j < min c.n 63 + 1 by omega]
  rw [Finset.sum_congr rfl hlow, hhigh]
  simp only [chunkDigest, ← hn]
  apply Prod.ext
  · simp only [Prod.fst_add, Prod.fst_sum, add_zero, Finset.sum_add_distrib]
  · simp only [Prod.snd_add, Prod.snd_sum, Finset.sum_add_distrib]
    split_ifs <;> ring

omit [CharP F 2]

/-- Replace one key entry. -/
noncomputable def setKey (K : KeyTable) (j : Fin 65) (l : Lane) (v : Word × Word) : KeyTable :=
  Function.update K j (Function.update (K j) l v)

lemma setKey_apply (K : KeyTable) (j : Fin 65) (l : Lane) (v : Word × Word)
    (j' : Fin 65) (l' : Lane) :
    setKey K j l v j' l' = if j' = j ∧ l' = l then v else K j' l' := by
  unfold setKey
  by_cases hj : j' = j
  · subst hj
    by_cases hl : l' = l
    · subst hl; simp
    · simp [hl]
  · simp [hj]

lemma keyAt_setKey (K : KeyTable) (j : Fin 65) (l : Lane) (v : Word × Word)
    (j' : ℕ) (l' : Lane) :
    keyAt (setKey K j l v) j' l' = if j' = j.val ∧ l' = l then v else keyAt K j' l' := by
  unfold keyAt
  by_cases hj : j' < 65
  · simp only [hj, dite_true, setKey_apply, Fin.ext_iff]
  · have : j' ≠ j.val := by have := j.isLt; omega
    simp [hj, this]

lemma sum_setKey {M : Type} [AddCommGroup M] (φ : ℕ → Lane → Word × Word → M)
    (K : KeyTable) (j : Fin 65) (l : Lane) (v : Word × Word) :
    ∑ j' ∈ Finset.range 65, ∑ l' : Lane, φ j' l' (keyAt (setKey K j l v) j' l') =
      ∑ j' ∈ Finset.range 65, ∑ l' : Lane, φ j' l' (keyAt K j' l') +
        (φ j l v - φ j l (keyAt K j l)) := by
  have h (j' : ℕ) (l' : Lane) : φ j' l' (keyAt (setKey K j l v) j' l') =
      φ j' l' (keyAt K j' l') +
        if j' = j.val then (if l' = l then φ j l v - φ j l (keyAt K j l) else 0) else 0 := by
    rw [keyAt_setKey]
    by_cases hj : j' = j.val
    · subst hj
      by_cases hl : l' = l
      · subst hl; simp
      · simp [hl]
    · simp [hj]
  simp only [h, Finset.sum_add_distrib]
  congr 1
  simp only [Finset.sum_ite_irrel, Finset.sum_const_zero]
  rw [Finset.sum_ite_eq' (Finset.range 65) j.val]
  simp [j.isLt]

/-- A table with two entries zeroed: the conditioning variables. -/
abbrev Rest (p q : Fin 65 × Lane) : Type :=
  {K : KeyTable // K p.1 p.2 = 0 ∧ K q.1 q.2 = 0}

/-- Split a table into its two selected entries and everything else. -/
noncomputable def splitEquiv (p q : Fin 65 × Lane) (hpq : p ≠ q) :
    KeyTable ≃ Rest p q × ((Word × Word) × (Word × Word)) where
  toFun K := (⟨setKey (setKey K p.1 p.2 0) q.1 q.2 0, by
      refine ⟨?_, ?_⟩ <;> simp only [setKey_apply] <;>
        [simp [show ¬ (p.1 = q.1 ∧ p.2 = q.2) from fun h => hpq (Prod.ext h.1 h.2)]; simp]⟩,
    (K p.1 p.2, K q.1 q.2))
  invFun z := setKey (setKey z.1.val p.1 p.2 z.2.1) q.1 q.2 z.2.2
  left_inv K := by
    funext j l
    simp only [setKey_apply]
    split_ifs <;> simp_all
  right_inv z := by
    obtain ⟨⟨K, hp, hq⟩, a, b⟩ := z
    have hpq' : ¬ (p.1 = q.1 ∧ p.2 = q.2) := fun h => hpq (Prod.ext h.1 h.2)
    refine Prod.ext (Subtype.ext ?_) (Prod.ext ?_ ?_)
    · funext j l
      change setKey (setKey (setKey (setKey K p.1 p.2 a) q.1 q.2 b) p.1 p.2 0) q.1 q.2 0 j l =
        K j l
      simp only [setKey_apply]
      split_ifs <;> simp_all
    · change setKey (setKey K p.1 p.2 a) q.1 q.2 b p.1 p.2 = a
      simp [setKey_apply, hpq']
    · change setKey (setKey K p.1 p.2 a) q.1 q.2 b q.1 q.2 = b
      simp [setKey_apply]

/-- Summing fiber bounds over a finite conditioning space. -/
lemma card_subtype_prod_le {A B : Type} [Finite A] [Finite B] (P : A × B → Prop) (M : ℕ)
    (h : ∀ a, Nat.card {b // P (a, b)} ≤ M) :
    Nat.card {z // P z} ≤ Nat.card A * M := by
  have := Fintype.ofFinite A
  rw [Nat.card_congr (Equiv.subtypeProdEquivSigmaSubtype (fun a b => P (a, b))),
    Nat.card_sigma]
  calc ∑ a, Nat.card {b // P (a, b)} ≤ ∑ _a : A, M := Finset.sum_le_sum fun a _ => h a
    _ = Nat.card A * M := by simp [Nat.card_eq_fintype_card]

/-- Any two columns with nonzero determinant determine both coefficients. -/
lemma mix_injective (w₁ w₂ : F × F) (hdet : w₁.1 * w₂.2 - w₁.2 * w₂.1 ≠ 0) :
    Function.Injective (fun uv : F × F => scale w₁ uv.1 + scale w₂ uv.2) := by
  rintro ⟨u, v⟩ ⟨u', v'⟩ h
  simp only [scale, Prod.mk_add_mk, Prod.mk.injEq] at h
  obtain ⟨h₁, h₂⟩ := h
  have hu : (w₁.1 * w₂.2 - w₁.2 * w₂.1) * (u - u') = 0 := by
    linear_combination w₂.2 * h₁ - w₂.1 * h₂
  have hv : (w₁.1 * w₂.2 - w₁.2 * w₂.1) * (v - v') = 0 := by
    linear_combination w₁.1 * h₂ - w₁.2 * h₁
  exact Prod.ext (sub_eq_zero.mp ((mul_eq_zero.mp hu).resolve_left hdet))
    (sub_eq_zero.mp ((mul_eq_zero.mp hv).resolve_left hdet))

lemma keyAt_fin (K : KeyTable) (j : Fin 65) (l : Lane) : keyAt K j.val l = K j l := by
  simp [keyAt, j.isLt]

/-- The digest difference, as a sum of independent local terms. -/
noncomputable def digestDiff (ι : R →+ F) (c c' : Chunk) (K : KeyTable) : F × F :=
  ∑ j ∈ Finset.range 65, ∑ l : Lane,
    scale (column ι c.n j) (laneDiff ι c c' j l (keyAt K j l))

/-- After conditioning on the rest of the table, the digest difference is a
    fixed offset plus the column mix of the two selected NH differences. -/
lemma digestDiff_split (ι : R →+ F) (c c' : Chunk) (p q : Fin 65 × Lane) (hpq : p ≠ q)
    (r : Rest p q) (ab : (Word × Word) × (Word × Word)) :
    digestDiff ι c c' ((splitEquiv p q hpq).symm (r, ab)) =
      (digestDiff ι c c' r.val
        - scale (column ι c.n p.1) (laneDiff ι c c' p.1 p.2 0)
        - scale (column ι c.n q.1) (laneDiff ι c c' q.1 q.2 0)) +
      (scale (column ι c.n p.1) (laneDiff ι c c' p.1 p.2 ab.1) +
        scale (column ι c.n q.1) (laneDiff ι c c' q.1 q.2 ab.2)) := by
  obtain ⟨K, hp, hq⟩ := r
  obtain ⟨a, b⟩ := ab
  have hpq' : ¬ (q.1.val = p.1.val ∧ q.2 = p.2) :=
    fun h => hpq (Prod.ext (Fin.ext h.1.symm) h.2.symm)
  change digestDiff ι c c' (setKey (setKey K p.1 p.2 a) q.1 q.2 b) = _
  unfold digestDiff
  rw [sum_setKey (fun j l v => scale (column ι c.n j) (laneDiff ι c c' j l v)),
    sum_setKey (fun j l v => scale (column ι c.n j) (laneDiff ι c c' j l v)),
    keyAt_setKey, keyAt_fin, keyAt_fin]
  simp only [hpq', ite_false, hp, hq]
  abel

lemma column_low (ι : R →+ F) {n j : ℕ} (h : j < min n 63 + 1) :
    column ι n j = (1, ι (bitPoly 6 j)) := by
  simp [column, h]

lemma column_final (ι : R →+ F) : column ι 64 64 = (0, 1) := by
  simp [column]

variable [CharP F 2]

/-- The count of key tables producing a given chunk digest difference:
    at most `2⁻¹²⁸` of the total, via two independent NH collisions. -/
lemma card_chunk_le (ι : R →+ F) (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (c c' : Chunk) (hn : c.n = c'.n)
    (hdiff : ∃ b, b < c.n ∧ c.blocks b ≠ c'.blocks b)
    (d : F × F) :
    Nat.card {K : KeyTable // chunkDigest ι c K + chunkDigest ι c' K = d}
      * 2 ^ 128 ≤ Nat.card KeyTable := by
  -- Lemma 2: a lane with two differing encoded positions.
  obtain ⟨l, j₁, j₂, hj, hj₂, h₁, h₂⟩ := ChainProof.chain_distance_two c c' hn hdiff 0
  have hcn := c.hn_le
  let p : Fin 65 × Lane := (⟨j₁, by omega⟩, l)
  let q : Fin 65 × Lane := (⟨j₂, by omega⟩, l)
  have hpq : p ≠ q := fun h => by
    have := congrArg (fun x : Fin 65 × Lane => x.1.val) h
    simp only [p, q] at this
    omega
  have hd (j : ℕ) (h : encodedPair c 0 j l ≠ encodedPair c' 0 j l) :
      ((dataPair c j l).1, (dataPair c j l).2) ≠ ((dataPair c' j l).1, (dataPair c' j l).2) := by
    intro he
    apply h
    rw [encodedPair_eq, encodedPair_eq]
    rw [Prod.mk.injEq] at he
    rw [he.1, he.2]
  -- Lemma 3: the two columns are independent.
  let w₁ := column ι c.n j₁
  let w₂ := column ι c.n j₂
  have hw₁ : w₁ = (1, ι (bitPoly 6 j₁)) := column_low ι (by omega)
  have hdet : w₁.1 * w₂.2 - w₁.2 * w₂.1 ≠ 0 := by
    by_cases h64 : j₂ < 64
    · have hw₂ : w₂ = (1, ι (bitPoly 6 j₂)) := column_low ι (by omega)
      rw [hw₁, hw₂]
      simp only [one_mul, mul_one]
      intro h
      have := map_bitPoly_injective ι hι (by norm_num)
        (show j₂ < 2 ^ 6 by omega) (show j₁ < 2 ^ 6 by omega) (sub_eq_zero.mp h)
      omega
    · have hc : c.n = 64 := by omega
      have hw₂ : w₂ = (0, 1) := by
        simp only [w₂, hc, show j₂ = 64 by omega]
        exact column_final ι
      rw [hw₁, hw₂]
      simp
  let mix := fun uv : F × F => scale w₁ uv.1 + scale w₂ uv.2
  have hmix : Function.Injective mix := mix_injective w₁ w₂ hdet
  -- Condition on every other table entry.
  let e := splitEquiv p q hpq
  let P : KeyTable → Prop := fun K => chunkDigest ι c K + chunkDigest ι c' K = d
  have hcount : Nat.card {K // P K} ≤ Nat.card (Rest p q) * 2 ^ 128 := by
    rw [Nat.card_congr (e.subtypeEquiv (p := P) (q := fun z => P (e.symm z))
      (fun K => by simp only [Equiv.symm_apply_apply]))]
    apply card_subtype_prod_le (fun z => P (e.symm z))
    intro r
    let off := digestDiff ι c c' r.val
      - scale (column ι c.n p.1) (laneDiff ι c c' p.1 p.2 0)
      - scale (column ι c.n q.1) (laneDiff ι c c' q.1 q.2 0)
    have hiff (ab : (Word × Word) × (Word × Word)) : P (e.symm (r, ab)) ↔
        mix (nhDifference ι (dataPair c j₁ l).1 (dataPair c j₁ l).2
            (dataPair c' j₁ l).1 (dataPair c' j₁ l).2 ab.1,
          nhDifference ι (dataPair c j₂ l).1 (dataPair c j₂ l).2
            (dataPair c' j₂ l).1 (dataPair c' j₂ l).2 ab.2) = d - off := by
      simp only [P]
      rw [digest_diff_eq ι c c' hn]
      change digestDiff ι c c' _ = d ↔ _
      rw [digestDiff_split ι c c' p q hpq r ab, eq_sub_iff_add_eq, add_comm]
      rfl
    rw [Nat.card_congr (Equiv.subtypeEquivRight hiff)]
    exact two_nh_count ι hι _ _ _ _ _ _ _ _ (hd j₁ h₁) (hd j₂ h₂) mix hmix (d - off)
  have htotal : Nat.card KeyTable = Nat.card (Rest p q) * 2 ^ 256 := by
    rw [Nat.card_congr e]
    simp only [Nat.card_prod, NHProof.card_Word]
    norm_num
  rw [htotal]
  calc Nat.card {K // P K} * 2 ^ 128 ≤ Nat.card (Rest p q) * 2 ^ 128 * 2 ^ 128 :=
        Nat.mul_le_mul_right _ hcount
    _ = Nat.card (Rest p q) * 2 ^ 256 := by ring

/-- **Proposition 4.** Single chunk is `2⁻¹²⁸`-AXU. -/
theorem chunk_axu
    (ι : R →+ F) (hι : ∀ x : R, x.degree < 128 → ι x = 0 → x = 0)
    (c c' : Chunk) (hn : c.n = c'.n)
    (hdiff : ∃ b, b < c.n ∧ c.blocks b ≠ c'.blocks b)
    (d : F × F) :
    (Nat.card {K : KeyTable //
      chunkDigest ι c K + chunkDigest ι c' K = d} : ℚ)
      / Nat.card KeyTable ≤ 1 / 2 ^ 128 := by
  have hpos : (0 : ℚ) < Nat.card KeyTable := by exact_mod_cast keyTable_card_pos
  rw [div_le_div_iff₀ hpos (by positivity), one_mul]
  exact_mod_cast card_chunk_le ι hι c c' hn hdiff d

end RaijuHash.ChunkProof

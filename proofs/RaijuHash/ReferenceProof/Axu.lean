import RaijuHash.ReferenceProof.Hash

/-!
# The AXU bound for the Rust reference, on raw key bytes

`splitKey` maps the 8432 key bytes read by `Params::from_bytes` bijectively
to the model key and the 16 bytes of `V`, which `hash` does not read. With
`hash_spec`, a uniformly random byte key therefore gives a uniformly random
model key, and Theorem 5 in `GF128` becomes `reference_hash_axu`: a bound
stated directly on the `u128` outputs of the transcribed `reference::hash`.
-/

set_option maxRecDepth 4096

namespace RaijuHash.Reference
open Polynomial RaijuHash

/-! ### Injectivity of the encodings -/

lemma fromLE_map_inj {n : ℕ} {f g : ℕ → ℕ} (hf : ∀ k < n, f k < 256) (hg : ∀ k < n, g k < 256)
    (h : fromLE ((List.range n).map f) = fromLE ((List.range n).map g)) :
    ∀ k < n, f k = g k := by
  have hbf : ∀ b ∈ (List.range n).map f, b < 256 := by
    intro b hb
    obtain ⟨k, hk, rfl⟩ := List.mem_map.mp hb
    exact hf k (List.mem_range.mp hk)
  have hbg : ∀ b ∈ (List.range n).map g, b < 256 := by
    intro b hb
    obtain ⟨k, hk, rfl⟩ := List.mem_map.mp hb
    exact hg k (List.mem_range.mp hk)
  intro k hk
  apply Nat.eq_of_testBit_eq
  intro t
  by_cases ht : t < 8
  · have := congrArg (fun x => x.testBit (8 * k + t)) h
    simp only [testBit_fromLE _ hbf, testBit_fromLE _ hbg,
      show (8 * k + t) / 8 = k by omega, show (8 * k + t) % 8 = t by omega] at this
    simpa [List.getD_eq_getElem?_getD, List.getElem?_map, List.getElem?_range, hk] using this
  · rw [testBit_of_lt_two_pow (n := 8) (by simpa using hf k hk) (by omega),
      testBit_of_lt_two_pow (n := 8) (by simpa using hg k hk) (by omega)]

lemma toWord_inj {a b : ℕ} (ha : a < 2 ^ 64) (hb : b < 2 ^ 64) (h : toWord a = toWord b) :
    a = b := by
  apply toPoly_injective
  rw [← toWord_coe ha, ← toWord_coe hb, h]

lemma fe_inj {a b : ℕ} (ha : a < 2 ^ 128) (hb : b < 2 ^ 128) (h : fe a = fe b) : a = b :=
  toPoly_injective (NHProof.map_inj_bounded ιGF ιGF_kernel (toPoly_degree ha) (toPoly_degree hb) h)

lemma byteOf_injective {a b : ℕ} (ha : a < 256) (hb : b < 256) (h : byteOf a = byteOf b) :
    a = b := by
  apply Nat.eq_of_testBit_eq
  intro t
  by_cases ht : t < 8
  · have := congrFun h ⟨t, ht⟩
    simp only [byteOf] at this
    cases h₁ : a.testBit t <;> cases h₂ : b.testBit t <;> simp_all
  · rw [testBit_of_lt_two_pow (n := 8) (by simpa using ha) (by omega),
      testBit_of_lt_two_pow (n := 8) (by simpa using hb) (by omega)]

lemma msgOf_injective {m m' : List ℕ} (hb : ∀ b ∈ m, b < 256) (hb' : ∀ b ∈ m', b < 256)
    (h : msgOf m = msgOf m') : m = m' := by
  have hl : m.length = m'.length := by
    simpa [msgOf] using congrArg List.length h
  apply List.ext_getElem hl
  intro i h₁ h₂
  have := congrArg (fun l => l[i]?) h
  simp only [msgOf, List.getElem?_map, List.getElem?_eq_getElem h₁,
    List.getElem?_eq_getElem h₂, Option.map_some, Option.some.injEq] at this
  exact byteOf_injective (hb _ (List.getElem_mem _)) (hb' _ (List.getElem_mem _)) this

/-! ### Keys as Rust stores them -/

/-- `KEY_BYTES`: the table, then `A, B, R, R2, T, S, V`. -/
abbrev KeyBytes : Type := Fin 8432 → Fin 256

/-- The byte function handed to `Params::from_bytes`. -/
def keyByte (kb : KeyBytes) (i : ℕ) : ℕ := if h : i < 8432 then (kb ⟨i, h⟩).val else 0

lemma keyByte_lt (kb : KeyBytes) (i : ℕ) : keyByte kb i < 256 := by
  unfold keyByte
  split_ifs
  · exact (kb _).isLt
  · norm_num

/-- `Params::from_bytes(kb)`. -/
def paramsOf (kb : KeyBytes) : Params := Params.fromBytes (keyByte kb)

lemma fromLE_range_lt (f : ℕ → ℕ) (hf : ∀ k, f k < 256) (n : ℕ) :
    fromLE ((List.range n).map f) < 2 ^ (8 * n) := by
  have := fromLE_lt ((List.range n).map f) (by
    intro b hb
    obtain ⟨k, -, rfl⟩ := List.mem_map.mp hb
    exact hf k)
  simpa using this

lemma paramsOf_wellFormed (kb : KeyBytes) : (paramsOf kb).WellFormed := by
  have h8 := fun j i => fromLE_range_lt (fun k => keyByte kb (128 * j + 8 * i + k))
    (fun _ => keyByte_lt _ _) 8
  have h16 := fun i => fromLE_range_lt (fun k => keyByte kb (tableBytes + 16 * i + k))
    (fun _ => keyByte_lt _ _) 16
  exact ⟨fun j i => h8 j.val i.val, h16 0, h16 1, h16 2, h16 3, h16 4, h16 5, h16 6⟩

/-- Field element `n` of the key: `A, B, R, R2, T, S, V` for `n = 0, …, 6`. -/
def fieldAt (kb : KeyBytes) (n : ℕ) : ℕ :=
  fromLE ((List.range 16).map fun k => keyByte kb (tableBytes + 16 * n + k))

/-- The model key used by `hash`, and the `V` bytes that `hash` ignores. -/
noncomputable def splitKey (kb : KeyBytes) : HashKey GF128 × (Fin 16 → Fin 256) :=
  (keyOf (paramsOf kb), fun k => kb ⟨8416 + k.val, by omega⟩)

lemma splitKey_injective : Function.Injective splitKey := by
  intro kb kb' h
  have hk : keyOf (paramsOf kb) = keyOf (paramsOf kb') := congrArg Prod.fst h
  have hv := congrArg Prod.snd h
  have hwf := paramsOf_wellFormed kb
  have hwf' := paramsOf_wellFormed kb'
  -- Table words.
  have hrow : ∀ j < 65, ∀ w < 16, (paramsOf kb).row j w = (paramsOf kb').row j w := by
    intro j hj w hw
    have ht := congrArg HashKey.table hk
    by_cases h8 : w < 8
    · have := congrArg (fun K : KeyTable => (K ⟨j, hj⟩ ⟨w, h8⟩).1) ht
      exact toWord_inj (row_lt hwf _ _) (row_lt hwf' _ _) this
    · have := congrArg (fun K : KeyTable => (K ⟨j, hj⟩ ⟨w - 8, by omega⟩).2) ht
      simp only [keyOf, keyTable, show 8 + (w - 8) = w by omega] at this
      exact toWord_inj (row_lt hwf _ _) (row_lt hwf' _ _) this
  -- Field elements.
  have hfield : ∀ n < 6, fieldAt kb n = fieldAt kb' n := by
    intro n hn
    interval_cases n
    · exact fe_inj hwf.2.1 hwf'.2.1 (congrArg HashKey.a hk)
    · exact fe_inj hwf.2.2.1 hwf'.2.2.1 (congrArg HashKey.b hk)
    · exact fe_inj hwf.2.2.2.1 hwf'.2.2.2.1 (congrArg HashKey.r hk)
    · exact fe_inj hwf.2.2.2.2.1 hwf'.2.2.2.2.1 (congrArg HashKey.r2 hk)
    · exact fe_inj hwf.2.2.2.2.2.1 hwf'.2.2.2.2.2.1 (congrArg HashKey.t hk)
    · exact fe_inj hwf.2.2.2.2.2.2.1 hwf'.2.2.2.2.2.2.1 (congrArg HashKey.s hk)
  have hbyte : ∀ i < 8432, keyByte kb i = keyByte kb' i := by
    intro i hi
    by_cases h1 : i < 8320
    · have hr := hrow (i / 128) (by omega) ((i % 128) / 8) (by omega)
      simp only [Params.row, show i / 128 < 65 ∧ (i % 128) / 8 < 16 by omega,
        paramsOf, Params.fromBytes] at hr
      have := fromLE_map_inj (fun _ _ => keyByte_lt _ _) (fun _ _ => keyByte_lt _ _) hr
        (i % 8) (by omega)
      rwa [show 128 * (i / 128) + 8 * (i % 128 / 8) + i % 8 = i by omega] at this
    · by_cases h2 : i < 8416
      · have := fromLE_map_inj (fun _ _ => keyByte_lt _ _) (fun _ _ => keyByte_lt _ _)
          (hfield ((i - 8320) / 16) (by omega)) ((i - 8320) % 16) (by omega)
        rwa [show tableBytes + 16 * ((i - 8320) / 16) + (i - 8320) % 16 = i by
          unfold tableBytes; omega] at this
      · have hvi := congrArg Fin.val (congrFun hv ⟨i - 8416, by omega⟩)
        have e : (⟨i, hi⟩ : Fin 8432) = ⟨8416 + (i - 8416), by omega⟩ := Fin.ext (by simp; omega)
        simp only [keyByte, hi, dite_true]
        rw [e]
        exact hvi
  funext i
  apply Fin.ext
  have := hbyte i.val i.isLt
  simpa [keyByte, i.isLt] using this

lemma card_keyBytes : Nat.card KeyBytes = 2 ^ (8 * 8432) := by
  rw [Nat.card_fun, Nat.card_eq_fintype_card, Nat.card_eq_fintype_card, Fintype.card_fin,
    Fintype.card_fin, show (256 : ℕ) = 2 ^ 8 by norm_num, ← pow_mul]

lemma natCard_GF128 : Nat.card GF128 = 2 ^ 128 := by
  rw [Nat.card_eq_fintype_card]
  exact card_GF128

lemma card_hashKey_gf128 : Nat.card (HashKey GF128) = 2 ^ (65 * 16 * 64 + 6 * 128) := by
  rw [Nat.card_congr (hashKeyEquiv GF128)]
  simp only [Nat.card_prod, card_KeyTable, natCard_GF128]
  simp only [← pow_add]

lemma card_splitKey : Nat.card KeyBytes = Nat.card (HashKey GF128 × (Fin 16 → Fin 256)) := by
  rw [Nat.card_prod, card_keyBytes, card_hashKey_gf128, Nat.card_fun, Nat.card_eq_fintype_card,
    Nat.card_eq_fintype_card, Fintype.card_fin, Fintype.card_fin,
    show (256 : ℕ) = 2 ^ 8 by norm_num, ← pow_mul, ← pow_add]

lemma splitKey_bijective : Function.Bijective splitKey :=
  (Nat.bijective_iff_injective_and_card splitKey).mpr ⟨splitKey_injective, card_splitKey⟩

/-- A bijection onto a product transfers probabilities of events that only
    depend on the first component. -/
lemma card_transfer {A B V : Type} [Finite A] [Finite V] [Nonempty V] (f : A → B × V)
    (hf : Function.Bijective f) (P : A → Prop) (Q : B → Prop) (hPQ : ∀ a, P a ↔ Q (f a).1) :
    (Nat.card {a // P a} : ℚ) / Nat.card A = Nat.card {b // Q b} / Nat.card B := by
  let e := Equiv.ofBijective f hf
  have hB : Finite (B × V) := Finite.of_equiv A e
  have : Finite B := Finite.of_injective (fun b => (b, Classical.arbitrary V))
    (fun _ _ h => congrArg Prod.fst h)
  have hcard : Nat.card {a // P a} = Nat.card {b // Q b} * Nat.card V := by
    rw [Nat.card_congr (e.subtypeEquiv (q := fun z => Q z.1) fun a => hPQ a),
      Nat.card_congr Equiv.prodSubtypeFstEquivSubtypeProd, Nat.card_prod]
  have hV : (0 : ℚ) < Nat.card V := by exact_mod_cast Nat.card_pos
  rw [hcard, Nat.card_congr e, Nat.card_prod, Nat.cast_mul, Nat.cast_mul,
    mul_div_mul_right _ _ hV.ne']

/-- **End-to-end bound for the Rust reference.** For uniformly random key
    bytes parsed by `Params::from_bytes`, distinct byte messages of at most
    `L < 2⁶⁴` bytes, and any 128-bit `d`, the transcription of
    `reference::hash` satisfies
    `Pr[hash(m) ^ hash(m') = d] ≤ (⌈L/8192⌉ + 1) / 2¹²⁸`. -/
theorem reference_hash_axu (L : ℕ) (m m' : List ℕ) (hb : ∀ b ∈ m, b < 256)
    (hb' : ∀ b ∈ m', b < 256) (hm : m ≠ m') (hL : m.length ≤ L) (hL' : m'.length ≤ L)
    (h64 : L < 2 ^ 64) (d : ℕ) :
    (Nat.card {kb : KeyBytes // hash (paramsOf kb) m ^^^ hash (paramsOf kb) m' = d} : ℚ)
      / Nat.card KeyBytes ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 := by
  have hm64 : m.length < 2 ^ 64 := hL.trans_lt h64
  have hm64' : m'.length < 2 ^ 64 := hL'.trans_lt h64
  have hspec := fun kb => hash_spec (paramsOf kb) (paramsOf_wellFormed kb) m hb hm64
  have hspec' := fun kb => hash_spec (paramsOf kb) (paramsOf_wellFormed kb) m' hb' hm64'
  by_cases hd : d < 2 ^ 128
  swap
  · have hE : ∀ kb : KeyBytes, ¬ (hash (paramsOf kb) m ^^^ hash (paramsOf kb) m' = d) :=
      fun kb h => hd (h ▸ Nat.xor_lt_two_pow (hspec kb).1 (hspec' kb).1)
    have : IsEmpty {kb : KeyBytes // hash (paramsOf kb) m ^^^ hash (paramsOf kb) m' = d} :=
      ⟨fun x => hE x.1 x.2⟩
    rw [Nat.card_of_isEmpty, Nat.cast_zero, zero_div]
    positivity
  have hiff (kb : KeyBytes) : (hash (paramsOf kb) m ^^^ hash (paramsOf kb) m' = d) ↔
      raijuhash ιGF (splitKey kb).1 (msgOf m) + raijuhash ιGF (splitKey kb).1 (msgOf m') =
        fe d := by
    change _ ↔ raijuhash ιGF (keyOf (paramsOf kb)) (msgOf m) +
      raijuhash ιGF (keyOf (paramsOf kb)) (msgOf m') = fe d
    rw [← (hspec kb).2, ← (hspec' kb).2, ← fe_xor]
    exact ⟨fun h => h ▸ rfl,
      fun h => fe_inj (Nat.xor_lt_two_pow (hspec kb).1 (hspec' kb).1) hd h⟩
  rw [card_transfer splitKey splitKey_bijective _
    (fun k => raijuhash ιGF k (msgOf m) + raijuhash ιGF k (msgOf m') = fe d) hiff]
  exact raijuhash_axu_gf128 L (msgOf m) (msgOf m') (fun h => hm (msgOf_injective hb hb' h))
    (by simpa [msgOf] using hL) (by simpa [msgOf] using hL') h64 (fe d)

end RaijuHash.Reference

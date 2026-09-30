import RaijuHash.Rabin
import RaijuHash.Frobenius
import RaijuHash.FullProof.Main

/-!
# The concrete field `GF(2¹²⁸)` of the Rust implementation

`fPoly = X¹²⁸ + X⁷ + X² + X + 1` is irreducible (`fPoly_irreducible`), so
`GF128 = 𝔽₂[X]/(fPoly)` is a field with `2¹²⁸` elements. The map `ιGF`
sends a polynomial of degree below 128 (a `u128` with bit `i` the
coefficient of `Xⁱ`) to its residue class, which is exactly how
`reference.rs` reads field elements. `raijuhash_axu_gf128` is Theorem 5 for
this field, with no remaining hypotheses about `F` or `ι`.
-/

namespace RaijuHash
open Polynomial

lemma isCoprime_of_bezout (a u v : ℕ) (h : clmulN u a ^^^ clmulN v fNat = 1) :
    IsCoprime (toPoly a) fPoly := by
  have hb := congrArg toPoly h
  rw [toPoly_xor, toPoly_clmulN, toPoly_clmulN, toPoly_fNat, toPoly_one] at hb
  exact ⟨_, _, hb⟩

lemma isCoprime_of_mk_eq {p : R} {a : ℕ} (hc : IsCoprime (toPoly a) fPoly)
    (h : AdjoinRoot.mk fPoly p = AdjoinRoot.mk fPoly (toPoly a)) : IsCoprime p fPoly := by
  obtain ⟨w, hw⟩ := AdjoinRoot.mk_eq_mk.mp h
  rw [sub_eq_iff_eq_add'.mp hw]
  exact hc.add_mul_left_left w

/-- `fPoly ∣ X^(2¹²⁸) - X`, from the checked squaring chain. -/
lemma fPoly_dvd_frobenius (N : ℕ) (hN : N = 2 ^ 128) : fPoly ∣ X ^ N - X := by
  have h := mk_squaringChain 2 frobChain frobenius_chain
  rw [frobChain_last, frobChain_length, toPoly_two, ← hN] at h
  rw [← AdjoinRoot.mk_eq_mk, map_pow]
  exact h.symm

/-- `X^(2⁶⁴) - X` is coprime to `fPoly`, from the checked chain prefix and
    the checked Bezout certificate. -/
lemma fPoly_coprime_frobenius (N : ℕ) (hN : N = 2 ^ 64) : IsCoprime (X ^ N - X) fPoly := by
  have h := mk_squaringChain 2 (frobChain.take 64) frobenius_chain64
  have hlen : (frobChain.take 64).length = 64 := by decide +kernel
  rw [frobChain64_last, toPoly_two, hlen, ← hN] at h
  apply isCoprime_of_mk_eq (isCoprime_of_bezout _ _ _ bezout64)
  rw [CharTwo.sub_eq_add, map_add, map_pow, ← h, toPoly_xor, map_add, toPoly_two]

lemma fPoly_monic : fPoly.Monic := by
  unfold fPoly
  monicity!

lemma fPoly_natDegree : fPoly.natDegree = 128 := by
  unfold fPoly
  compute_degree!

lemma fPoly_ne_zero : fPoly ≠ 0 := fPoly_monic.ne_zero

theorem fPoly_irreducible : Irreducible fPoly :=
  irreducible_of_frobenius fPoly_monic fPoly_natDegree _ _ rfl rfl
    (fPoly_dvd_frobenius _ rfl) (fPoly_coprime_frobenius _ rfl)

instance : Fact (Irreducible fPoly) := ⟨fPoly_irreducible⟩

/-- `GF(2¹²⁸)` with the modulus of `gf_mul` in `reference.rs`. -/
abbrev GF128 : Type := AdjoinRoot fPoly

instance : Module.Finite (ZMod 2) GF128 := (AdjoinRoot.powerBasis fPoly_ne_zero).finite

instance : Finite GF128 := Module.finite_of_finite (ZMod 2)

noncomputable instance : Fintype GF128 := Fintype.ofFinite _

theorem card_GF128 : Fintype.card GF128 = 2 ^ 128 := by
  rw [Module.card_eq_pow_finrank (K := ZMod 2), ZMod.card,
    (AdjoinRoot.powerBasis fPoly_ne_zero).finrank, AdjoinRoot.powerBasis_dim, fPoly_natDegree]

instance : CharP GF128 2 :=
  charP_of_injective_algebraMap (algebraMap (ZMod 2) GF128).injective 2

/-- A 128-bit value, read as a polynomial, to its field element. -/
noncomputable def ιGF : R →+ GF128 := (AdjoinRoot.mk fPoly).toAddMonoidHom

lemma ιGF_apply (p : R) : ιGF p = AdjoinRoot.mk fPoly p := rfl

theorem ιGF_kernel (x : R) (hx : x.degree < 128) (h : ιGF x = 0) : x = 0 := by
  apply eq_zero_of_dvd_of_degree_lt (AdjoinRoot.mk_eq_zero.mp h)
  rwa [degree_eq_natDegree fPoly_ne_zero, fPoly_natDegree]

/-- **Theorem 5 in Rust's field.** No hypotheses about the field or the
    embedding remain. -/
theorem raijuhash_axu_gf128 (L : ℕ) (m m' : List Byte) (hm : m ≠ m')
    (hL : m.length ≤ L) (hL' : m'.length ≤ L) (h64 : L < 2 ^ 64) (d : GF128) :
    (Nat.card {k : HashKey GF128 // raijuhash ιGF k m + raijuhash ιGF k m' = d} : ℚ)
      / Nat.card (HashKey GF128) ≤ (((L + 8191) / 8192 : ℕ) + 1 : ℚ) / 2 ^ 128 :=
  FullProof.raijuhash_axu GF128 card_GF128 ιGF ιGF_kernel L m m' hm hL hL' h64 d

end RaijuHash

//! The AArch64 block kernels, as inline assembly built from assembler macros.
//!
//! The bulk loop needs all 32 vector registers; compiled intrinsics spill
//! accumulators (8–12% slower, `docs/research/RESULTS.md`), so the loops are
//! written in assembly. Each `asm!` block defines the macros below, expands
//! them, and purges them again, so a block that the compiler duplicates or
//! inlines never redefines a macro. Macro arguments are register numbers
//! (`25` means `v25`/`q25`/`d25`) or general registers from `asm!` operands.
//!
//! A group is eight consecutive blocks at positions `8u + v`. For each block
//! the kernels form the product sum `S` and add it to `B[v]` and to the group
//! sum `C`; at the end of a group `C` is added to the planes `F[b]` for the
//! bits `b` set in `u`. Nothing is stored inside a loop, so no store can
//! alias the streaming loads (a store whose page offset the following loads
//! approach stalls them on Apple cores).
//!
//! One-chunk register plan (`bulk`, `groups`, `tail`):
//!
//! ```text
//! v0..v7    B[v]                   v8        C (current group)
//! v9..v16   rows of one block (x0..x3, y0..y3): set A; v17..v24 set B.
//!           The sets swap every block, so no moves are needed.
//! v25, v26  key words, then T and U; v25 then receives one product
//! v27       the other product      v28       S, the block's product sum
//! v29..v31  F[0..3]
//! ```
//!
//! Every kernel exists twice: with `EOR3` (FEAT_SHA3) and, for cores
//! without it, with each three-way XOR written as two `EOR`s. Only the
//! definition of `rj_x3` differs.

/// Three-way XOR with FEAT_SHA3.
macro_rules! x3_eor3 {
    (def) => {
        r"
.macro rj_x3 d, a, b, c
    eor3 v\d\().16b, v\a\().16b, v\b\().16b, v\c\().16b
.endm
"
    };
    (purge) => {
        ".purgem rj_x3"
    };
}

/// Three-way XOR as two `EOR`s, ordered so that an operand aliased with the
/// destination is read first.
macro_rules! x3_plain {
    (def) => {
        r"
.macro rj_x3 d, a, b, c
.ifc \d, \c
    eor v\d\().16b, v\c\().16b, v\a\().16b
    eor v\d\().16b, v\d\().16b, v\b\().16b
.else
.ifc \d, \b
    eor v\d\().16b, v\b\().16b, v\a\().16b
    eor v\d\().16b, v\d\().16b, v\c\().16b
.else
    eor v\d\().16b, v\a\().16b, v\b\().16b
    eor v\d\().16b, v\d\().16b, v\c\().16b
.endif
.endif
.endm
"
    };
    (purge) => {
        ".purgem rj_x3"
    };
}

/// Positions, blocks, groups and planes of the one-chunk plan (`bulk`, `groups`, `tail`).
macro_rules! rj_core {
    (def) => {
        concat!(
            // One lane pair q of a position: key words at koff + 16q (X) and + 64 (Y),
            // previous rows px/py, current rows cx/cy; the product sum goes to v28.
            r"
.macro rj_posq k, koff, q, px, py, cx, cy
    ldr q25, [\k, #(\koff + 16 * \q)]
    ldr q26, [\k, #(\koff + 64 + 16 * \q)]
    rj_x3 25, \px, \cx, 25
    rj_x3 26, \py, \cy, 26
    rj_posq_sum \q
.endm
",
            // The same for the end position, which has no current rows.
            r"
.macro rj_posq_end k, koff, q, px, py
    ldr q25, [\k, #(\koff + 16 * \q)]
    ldr q26, [\k, #(\koff + 64 + 16 * \q)]
    eor v25.16b, v25.16b, v\px\().16b
    eor v26.16b, v26.16b, v\py\().16b
    rj_posq_sum \q
.endm
",
            r"
.macro rj_posq_sum q
    pmull v27.1q, v25.1d, v26.1d
    pmull2 v25.1q, v25.2d, v26.2d
.if \q == 0
    eor v28.16b, v27.16b, v25.16b
.else
    rj_x3 28, 28, 27, 25
.endif
.endm
",
            // A position whose previous block is in set A (v9..v16) and current block
            // in set B (v17..v24), and the reverse.
            r"
.macro rj_pos_ab k, koff
    rj_posq \k, \koff, 0, 9, 13, 17, 21
    rj_posq \k, \koff, 1, 10, 14, 18, 22
    rj_posq \k, \koff, 2, 11, 15, 19, 23
    rj_posq \k, \koff, 3, 12, 16, 20, 24
.endm
",
            r"
.macro rj_pos_ba k, koff
    rj_posq \k, \koff, 0, 17, 21, 9, 13
    rj_posq \k, \koff, 1, 18, 22, 10, 14
    rj_posq \k, \koff, 2, 19, 23, 11, 15
    rj_posq \k, \koff, 3, 20, 24, 12, 16
.endm
",
            // The end position after a block in set A or in set B.
            r"
.macro rj_end_a k, koff
    rj_posq_end \k, \koff, 0, 9, 13
    rj_posq_end \k, \koff, 1, 10, 14
    rj_posq_end \k, \koff, 2, 11, 15
    rj_posq_end \k, \koff, 3, 12, 16
.endm
",
            r"
.macro rj_end_b k, koff
    rj_posq_end \k, \koff, 0, 17, 21
    rj_posq_end \k, \koff, 1, 18, 22
    rj_posq_end \k, \koff, 2, 19, 23
    rj_posq_end \k, \koff, 3, 20, 24
.endm
",
            // The block at in-group position v, data at d + off: its sum goes to B[v]
            // and C. Even blocks load into set B, odd blocks into set A.
            r"
.macro rj_block d, k, off, v
.if \v % 2 == 0
    ldp q17, q18, [\d, #(\off)]
    ldp q19, q20, [\d, #(\off + 32)]
    ldp q21, q22, [\d, #(\off + 64)]
    ldp q23, q24, [\d, #(\off + 96)]
    rj_pos_ab \k, (128 * \v)
.else
    ldp q9, q10, [\d, #(\off)]
    ldp q11, q12, [\d, #(\off + 32)]
    ldp q13, q14, [\d, #(\off + 64)]
    ldp q15, q16, [\d, #(\off + 96)]
    rj_pos_ba \k, (128 * \v)
.endif
    eor v\v\().16b, v\v\().16b, v28.16b
    eor v8.16b, v8.16b, v28.16b
.endm
",
            // Eight blocks at d, keyed by the rows at k.
            r"
.macro rj_group d, k
.irp v, 0, 1, 2, 3, 4, 5, 6, 7
    rj_block \d, \k, (128 * \v), \v
.endr
.endm
",
            // Add C to F[b] for each bit b of the group index u; start the next group.
            r"
.macro rj_planes u
    tbz \u, #0, 30f
    eor v29.16b, v29.16b, v8.16b
30:
    tbz \u, #1, 31f
    eor v30.16b, v30.16b, v8.16b
31:
    tbz \u, #2, 32f
    eor v31.16b, v31.16b, v8.16b
32:
    movi v8.16b, #0
    add \u, \u, #1
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_posq\n.purgem rj_posq_end\n.purgem rj_posq_sum\n.purgem rj_pos_ab\n.purgem rj_pos_ba\n.purgem rj_end_a\n.purgem rj_end_b\n.purgem rj_block\n.purgem rj_group\n.purgem rj_planes"
    };
}

/// The `h1` bit-plane fold and the outer step, shared by the bulk kernels.
macro_rules! rj_fold {
    (def) => {
        concat!(
            // z0 += sum_k x^k z_k for z_1..z_5 = (z1, z2, f0, f1, f2), shifted into
            // s/u and the carries above x^128 folded back with e0/e1.
            r"
.macro rj_h1 t, z0, z1, z2, f0, f1, f2, s0, s1, s2, s3, s4, u0, u1, u2, u3, u4, zr, poly, e0, e1
    shl v\s0\().2d, v\z1\().2d, #1
    ushr v\u0\().2d, v\z1\().2d, #63
    shl v\s1\().2d, v\z2\().2d, #2
    ushr v\u1\().2d, v\z2\().2d, #62
    shl v\s2\().2d, v\f0\().2d, #3
    ushr v\u2\().2d, v\f0\().2d, #61
    shl v\s3\().2d, v\f1\().2d, #4
    ushr v\u3\().2d, v\f1\().2d, #60
    shl v\s4\().2d, v\f2\().2d, #5
    ushr v\u4\().2d, v\f2\().2d, #59
    rj_x3 \z0, \z0, \s0, \s1
    rj_x3 \z0, \z0, \s2, \s3
    eor v\z0\().16b, v\z0\().16b, v\s4\().16b
    rj_x3 \u0, \u0, \u1, \u2
    rj_x3 \u0, \u0, \u3, \u4
    movi v\zr\().16b, #0
    mov \t, #0x87
    dup v\poly\().2d, \t
    ext v\e0\().16b, v\zr\().16b, v\u0\().16b, #8
    ext v\e1\().16b, v\u0\().16b, v\zr\().16b, #8
    pmull v\e1\().1q, v\e1\().1d, v\poly\().1d
    rj_x3 \z0, \z0, \e0, \e1
.endm
",
            // Outer step P = (P + h0) R + h1 R2, schoolbook with half-swapped keys at
            // r; P enters and leaves in plo/phi (no store in the loop).
            r"
.macro rj_outer r, plo, phi, h0, h1, p, kr, krs, kr2, kr2s, a0, a1, a2, a3, a4, a5, a6, a7, zr, poly
    fmov d\p, \plo
    mov v\p\().d[1], \phi
    ldp q\kr, q\krs, [\r]
    ldp q\kr2, q\kr2s, [\r, #32]
    eor v\h0\().16b, v\h0\().16b, v\p\().16b
    pmull v\a0\().1q, v\h0\().1d, v\kr\().1d
    pmull v\a1\().1q, v\h1\().1d, v\kr2\().1d
    pmull2 v\a2\().1q, v\h0\().2d, v\kr\().2d
    pmull2 v\a3\().1q, v\h1\().2d, v\kr2\().2d
    pmull v\a4\().1q, v\h0\().1d, v\krs\().1d
    pmull2 v\a5\().1q, v\h0\().2d, v\krs\().2d
    pmull v\a6\().1q, v\h1\().1d, v\kr2s\().1d
    pmull2 v\a7\().1q, v\h1\().2d, v\kr2s\().2d
    eor v\a0\().16b, v\a0\().16b, v\a1\().16b
    eor v\a2\().16b, v\a2\().16b, v\a3\().16b
    rj_x3 \a4, \a4, \a5, \a6
    eor v\a4\().16b, v\a4\().16b, v\a7\().16b
    ext v\a5\().16b, v\zr\().16b, v\a4\().16b, #8
    ext v\a6\().16b, v\a4\().16b, v\zr\().16b, #8
    eor v\a0\().16b, v\a0\().16b, v\a5\().16b
    eor v\a2\().16b, v\a2\().16b, v\a6\().16b
    pmull2 v\a5\().1q, v\a2\().2d, v\poly\().2d
    ext v\a6\().16b, v\a5\().16b, v\zr\().16b, #8
    ext v\a5\().16b, v\zr\().16b, v\a5\().16b, #8
    eor v\a2\().16b, v\a2\().16b, v\a6\().16b
    eor v\a0\().16b, v\a0\().16b, v\a5\().16b
    pmull v\a5\().1q, v\a2\().1d, v\poly\().1d
    eor v\a0\().16b, v\a0\().16b, v\a5\().16b
    fmov \plo, d\a0
    mov \phi, v\a0\().d[1]
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_h1\n.purgem rj_outer"
    };
}

/// The chunk fold of `bulk`.
macro_rules! rj_one {
    (def) => {
        concat!(
            // Chunk fold of the one-chunk plan and the outer step. In: B v0..v7,
            // F v29..v31, E v28; h0 = XOR of B, z0 = E + G0, z1 = G1, z2 = G2.
            r"
.macro rj_fold_one r, plo, phi, t
    rj_x3 9, 0, 1, 2
    rj_x3 9, 9, 3, 4
    rj_x3 9, 9, 5, 6
    eor v9.16b, v9.16b, v7.16b
    rj_x3 10, 1, 3, 5
    rj_x3 10, 10, 7, 28
    rj_x3 11, 2, 3, 6
    eor v11.16b, v11.16b, v7.16b
    rj_x3 12, 4, 5, 6
    eor v12.16b, v12.16b, v7.16b
    rj_h1 \t, 10, 11, 12, 29, 30, 31, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26
    rj_outer \r, \plo, \phi, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 25, 23, 24
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_fold_one"
    };
}

/// Pieces shared by the two-chunk kernels `bulk2` and `bulk4`.
macro_rules! rj_mix {
    (def) => {
        concat!(
            // Add T = v31 to the planes: F0 always, F1 if bit 0 of u, F2 if bits 0 and 1
            // (the telescoped planes of the two-chunk kernels).
            r"
.macro rj_tplanes u, f0, f1, f2
    eor v\f0\().16b, v\f0\().16b, v31.16b
    tbz \u, #0, 1f
    eor v\f1\().16b, v\f1\().16b, v31.16b
    tbz \u, #1, 1f
    eor v\f2\().16b, v\f2\().16b, v31.16b
1:
.endm
",
            // Both chunks' product for lane pair (x, y) into acc: pmull into x.
            r"
.macro rj_pprod acc, x, y
    pmull2 v31.1q, v\x\().2d, v\y\().2d
    pmull v\x\().1q, v\x\().1d, v\y\().1d
    rj_x3 \acc, \acc, \x, 31
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_tplanes\n.purgem rj_pprod"
    };
}

/// The two-chunk kernel `bulk2`, one lane pair per pass.
macro_rules! rj_pair {
    (def) => {
        concat!(
            // h0 and z0..z2 from the eight sums B[v] (no end position term).
            r"
.macro rj_bsums b0, b1, b2, b3, b4, b5, b6, b7, h0, z0, z1, z2
    rj_x3 \h0, \b0, \b1, \b2
    rj_x3 \h0, \h0, \b3, \b4
    rj_x3 \h0, \h0, \b5, \b6
    eor v\h0\().16b, v\h0\().16b, v\b7\().16b
    rj_x3 \z0, \b1, \b3, \b5
    eor v\z0\().16b, v\z0\().16b, v\b7\().16b
    rj_x3 \z1, \b2, \b3, \b6
    eor v\z1\().16b, v\z1\().16b, v\b7\().16b
    rj_x3 \z2, \b4, \b5, \b6
    eor v\z2\().16b, v\z2\().16b, v\b7\().16b
.endm
",
            // bulk2: lane pair q of position 8u + v for chunks A (at d) and B (at
            // d + 8192). pa/pb hold the previous block's pair and receive T, U and a
            // product; ca/cb receive the current pair; the key word is in v30.
            r"
.macro rj_ppos d, k, v, q, accb, pa0, pa1, ca0, ca1, pb0, pb1, cb0, cb1
    ldr q\ca0, [\d, #(128 * \v + 16 * \q)]
    ldr q\ca1, [\d, #(128 * \v + 16 * \q + 64)]
    ldr q\cb0, [\d, #(8192 + 128 * \v + 16 * \q)]
    ldr q\cb1, [\d, #(8192 + 128 * \v + 16 * \q + 64)]
    ldr q30, [\k, #(128 * \v + 16 * \q)]
    rj_x3 \pa0, \pa0, \ca0, 30
    rj_x3 \pb0, \pb0, \cb0, 30
    ldr q30, [\k, #(128 * \v + 16 * \q + 64)]
    rj_x3 \pa1, \pa1, \ca1, 30
    rj_x3 \pb1, \pb1, \cb1, 30
    rj_pprod \v, \pa0, \pa1
    rj_pprod \accb, \pb0, \pb1
.endm
",
            // Even positions read into (24, 25)/(28, 29), odd ones into (22, 23)/(26, 27).
            r"
.macro rj_ppos2 d, k, v, q, accb
.if \v % 2 == 0
    rj_ppos \d, \k, \v, \q, \accb, 22, 23, 24, 25, 26, 27, 28, 29
.else
    rj_ppos \d, \k, \v, \q, \accb, 24, 25, 22, 23, 28, 29, 26, 27
.endif
.endm
",
            // bulk2: one group of both chunks, as four lane-pair passes, then the planes.
            r"
.macro rj_pgroup d, k, u, z, t, pa, pb
    sub \t, \d, #128
    cmp \u, #0
    csel \pa, \z, \t, eq
    add \t, \d, #8192
    sub \t, \t, #128
    csel \pb, \z, \t, eq
.irp q, 0, 1, 2, 3
    ldr q22, [\pa, #(16 * \q)]
    ldr q23, [\pa, #(64 + 16 * \q)]
    ldr q26, [\pb, #(16 * \q)]
    ldr q27, [\pb, #(64 + 16 * \q)]
    rj_ppos2 \d, \k, 0, \q, 8
    rj_ppos2 \d, \k, 1, \q, 9
    rj_ppos2 \d, \k, 2, \q, 10
    rj_ppos2 \d, \k, 3, \q, 11
    rj_ppos2 \d, \k, 4, \q, 12
    rj_ppos2 \d, \k, 5, \q, 13
    rj_ppos2 \d, \k, 6, \q, 14
    rj_ppos2 \d, \k, 7, \q, 15
.endr
    rj_x3 31, 0, 1, 2
    rj_x3 31, 31, 3, 4
    rj_x3 31, 31, 5, 6
    eor v31.16b, v31.16b, v7.16b
    rj_tplanes \u, 16, 17, 18
    rj_x3 31, 8, 9, 10
    rj_x3 31, 31, 11, 12
    rj_x3 31, 31, 13, 14
    eor v31.16b, v31.16b, v15.16b
    rj_tplanes \u, 19, 20, 21
.endm
",
            // bulk2: end position 64 of both chunks (d at A's end, k at row 64); E is
            // added to B[0] and B[1] of each chunk.
            r"
.macro rj_pend d, k
.irp q, 0, 1, 2, 3
    ldur q22, [\d, #(-128 + 16 * \q)]
    ldur q23, [\d, #(-64 + 16 * \q)]
    ldr q26, [\d, #(8064 + 16 * \q)]
    ldr q27, [\d, #(8128 + 16 * \q)]
    ldr q30, [\k, #(16 * \q)]
    eor v22.16b, v22.16b, v30.16b
    eor v26.16b, v26.16b, v30.16b
    ldr q30, [\k, #(64 + 16 * \q)]
    eor v23.16b, v23.16b, v30.16b
    eor v27.16b, v27.16b, v30.16b
    pmull2 v31.1q, v22.2d, v23.2d
    pmull v22.1q, v22.1d, v23.1d
    rj_x3 0, 0, 22, 31
    rj_x3 1, 1, 22, 31
    pmull2 v31.1q, v26.2d, v27.2d
    pmull v26.1q, v26.1d, v27.1d
    rj_x3 8, 8, 26, 31
    rj_x3 9, 9, 26, 31
.endr
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_bsums\n.purgem rj_ppos\n.purgem rj_ppos2\n.purgem rj_pgroup\n.purgem rj_pend"
    };
}

/// The two-chunk kernel `bulk4`, two lane pairs per pass.
macro_rules! rj_quad {
    (def) => {
        concat!(
            // bulk4: one key word applied to a row of both chunks.
            r"
.macro rj_qkey k, koff, pa, ca, pb, cb
    ldr q30, [\k, #(\koff)]
    rj_x3 \pa, \pa, \ca, 30
    rj_x3 \pb, \pb, \cb, 30
.endm
",
            r"
.macro rj_qkey_end k, koff, pa, pb
    ldr q30, [\k, #(\koff)]
    eor v\pa\().16b, v\pa\().16b, v30.16b
    eor v\pb\().16b, v\pb\().16b, v30.16b
.endm
",
            // bulk4: the two lane pairs' products summed into x0.
            r"
.macro rj_qmix x0, y0, x1, y1
    pmull2 v31.1q, v\x0\().2d, v\y0\().2d
    pmull v\x0\().1q, v\x0\().1d, v\y0\().1d
    eor v\x0\().16b, v\x0\().16b, v31.16b
    pmull2 v31.1q, v\x1\().2d, v\y1\().2d
    pmull v\x1\().1q, v\x1\().1d, v\y1\().1d
    rj_x3 \x0, \x0, \x1, 31
.endm
",
            // bulk4: products of one chunk at position 8u + v into its accumulators
            // E0/E1 (by the parity of v), P1 (bit 1 of v) and P2 (bit 2 of v).
            r"
.macro rj_qprod v, x0, y0, x1, y1, e0, e1, p1, p2
.if (\v & 6) == 0
.if \v & 1
    rj_pprod \e1, \x0, \y0
    rj_pprod \e1, \x1, \y1
.else
    rj_pprod \e0, \x0, \y0
    rj_pprod \e0, \x1, \y1
.endif
.else
    rj_qmix \x0, \y0, \x1, \y1
.if \v & 1
    eor v\e1\().16b, v\e1\().16b, v\x0\().16b
.else
    eor v\e0\().16b, v\e0\().16b, v\x0\().16b
.endif
.if \v & 2
    eor v\p1\().16b, v\p1\().16b, v\x0\().16b
.endif
.if \v & 4
    eor v\p2\().16b, v\p2\().16b, v\x0\().16b
.endif
.endif
.endm
",
            // bulk4: lane pairs 2p and 2p + 1 of position 8u + v for both chunks; rows
            // are (x_2p, y_2p, x_2p+1, y_2p+1). With pf, also prefetch into L2 one line
            // of the group at f: each group's 16 lines are covered by its 16 passes.
            r"
.macro rj_qpos d, db, k, f, pf, v, p, pa0, pa1, pa2, pa3, ca0, ca1, ca2, ca3, pb0, pb1, pb2, pb3, cb0, cb1, cb2, cb3
.if \pf
    prfm pldl2keep, [\f, #(128 * \v + 8192 * \p)]
.endif
    ldp q\ca0, q\ca2, [\d, #(128 * \v + 32 * \p)]
    ldp q\ca1, q\ca3, [\d, #(128 * \v + 32 * \p + 64)]
    ldp q\cb0, q\cb2, [\db, #(128 * \v + 32 * \p)]
    ldp q\cb1, q\cb3, [\db, #(128 * \v + 32 * \p + 64)]
    rj_qkey \k, (128 * \v + 32 * \p), \pa0, \ca0, \pb0, \cb0
    rj_qkey \k, (128 * \v + 64 + 32 * \p), \pa1, \ca1, \pb1, \cb1
    rj_qkey \k, (128 * \v + 32 * \p + 16), \pa2, \ca2, \pb2, \cb2
    rj_qkey \k, (128 * \v + 64 + 32 * \p + 16), \pa3, \ca3, \pb3, \cb3
    rj_qprod \v, \pa0, \pa1, \pa2, \pa3, 0, 1, 2, 3
    rj_qprod \v, \pb0, \pb1, \pb2, \pb3, 7, 8, 9, 10
.endm
",
            // Rows of A in v14..v17 / v18..v21, of B in v22..v25 / v26..v29, swapping
            // every position.
            r"
.macro rj_qpos2 d, db, k, f, pf, v, p
.if \v % 2 == 0
    rj_qpos \d, \db, \k, \f, \pf, \v, \p, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29
.else
    rj_qpos \d, \db, \k, \f, \pf, \v, \p, 18, 19, 20, 21, 14, 15, 16, 17, 26, 27, 28, 29, 22, 23, 24, 25
.endif
.endm
",
            // bulk4: one group of both chunks as two passes over adjacent lane pairs.
            r"
.macro rj_qgroup d, db, k, f, pf, u, z, t, pa, pb
.if \pf
    add \f, \d, #8, lsl #12
.endif
    add \db, \d, #8192
    sub \t, \d, #128
    cmp \u, #0
    csel \pa, \z, \t, eq
    add \t, \d, #8192
    sub \t, \t, #128
    csel \pb, \z, \t, eq
.irp p, 0, 1
    ldp q14, q16, [\pa, #(32 * \p)]
    ldp q15, q17, [\pa, #(64 + 32 * \p)]
    ldp q22, q24, [\pb, #(32 * \p)]
    ldp q23, q25, [\pb, #(64 + 32 * \p)]
.irp v, 0, 1, 2, 3, 4, 5, 6, 7
    rj_qpos2 \d, \db, \k, \f, \pf, \v, \p
.endr
.endr
    eor v31.16b, v0.16b, v1.16b
    rj_tplanes \u, 4, 5, 6
    eor v31.16b, v7.16b, v8.16b
    rj_tplanes \u, 11, 12, 13
.endm
",
            // bulk4: end position 64 (d at A's end, t at B's end, k at row 64); E is
            // added to E0 and E1 of each chunk.
            r"
.macro rj_qend d, k, t
.irp p, 0, 1
    ldur q14, [\d, #(-128 + 32 * \p)]
    ldur q15, [\d, #(-64 + 32 * \p)]
    ldr q22, [\t, #(32 * \p)]
    ldr q23, [\t, #(64 + 32 * \p)]
    ldur q16, [\d, #(-128 + 32 * \p + 16)]
    ldur q17, [\d, #(-64 + 32 * \p + 16)]
    ldr q24, [\t, #(32 * \p + 16)]
    ldr q25, [\t, #(64 + 32 * \p + 16)]
    rj_qkey_end \k, (32 * \p), 14, 22
    rj_qkey_end \k, (64 + 32 * \p), 15, 23
    rj_qkey_end \k, (32 * \p + 16), 16, 24
    rj_qkey_end \k, (64 + 32 * \p + 16), 17, 25
    rj_qmix 14, 15, 16, 17
    eor v0.16b, v0.16b, v14.16b
    eor v1.16b, v1.16b, v14.16b
    rj_qmix 22, 23, 24, 25
    eor v7.16b, v7.16b, v22.16b
    eor v8.16b, v8.16b, v22.16b
.endr
.endm
",
            // bulk4: fold of one chunk from E0 E1 P1 P2 F0 F1 F2 = e0..f2, then the
            // outer step; h0 = E0 + E1, z0 = E1, z1 = P1, z2 = P2.
            r"
.macro rj_qfold r, plo, phi, t, e0, e1, p1, p2, f0, f1, f2
    eor v\e0\().16b, v\e0\().16b, v\e1\().16b
    mov v14.16b, v\e1\().16b
    rj_h1 \t, 14, \p1, \p2, \f0, \f1, \f2, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 15, 16
    rj_outer \r, \plo, \phi, \e0, 14, 17, 18, 19, 21, 22, 27, 28, 15, 16, 20, 23, 24, 17, 25, 26
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_qkey\n.purgem rj_qkey_end\n.purgem rj_qmix\n.purgem rj_qprod\n.purgem rj_qpos\n.purgem rj_qpos2\n.purgem rj_qgroup\n.purgem rj_qend\n.purgem rj_qfold"
    };
}

/// The final partial chunk (`tail`).
macro_rules! rj_tail {
    (def) => {
        concat!(
            // Clear one register. (Inside a macro body, `v\i\().16b` with an `.irp`
            // variable would lose its `\()` before the `.irp` expands.)
            r"
.macro rj_zero r
    movi v\r\().16b, #0
.endm
",
            // tail: the end position at in-group position npos (0..=8) after the last
            // block; leaves E in v28 (zero unless the end position is 64) and the
            // planes updated.
            // The end position opens group u + 1, or is position 64 itself.
            r"
.macro rj_tail_end k, u, npos
.if \npos % 2 == 0
    rj_end_a \k, (128 * \npos)
.else
    rj_end_b \k, (128 * \npos)
.endif
.if \npos < 8
    eor v\npos\().16b, v\npos\().16b, v28.16b
    eor v8.16b, v8.16b, v28.16b
    rj_planes \u
    movi v28.16b, #0
.else
    rj_planes \u
    cmp \u, #8
    b.eq 60f
    eor v0.16b, v0.16b, v28.16b
    tbz \u, #0, 50f
    eor v29.16b, v29.16b, v28.16b
50:
    tbz \u, #1, 51f
    eor v30.16b, v30.16b, v28.16b
51:
    tbz \u, #2, 52f
    eor v31.16b, v31.16b, v28.16b
52:
    movi v28.16b, #0
60:
.endif
.endm
",
            // tail: after n whole blocks of the last group, the zero-padded final block
            // (if last is not null) and the end position.
            r"
.macro rj_tail_case k, u, last, n, n1
7\n:
    cbz \last, 8\n\()f
    rj_block \last, \k, 0, \n
    rj_tail_end \k, \u, \n1
    b 9f
8\n:
    rj_tail_end \k, \u, \n
    b 9f
.endm
",
            // tail: one term a * (K, K half-swapped) into accumulators (lo, hi, mid).
            r"
.macro rj_term a, ptr, off, k, ks, t1, t2, t3, t4, lo, hi, mid
    ldp q\k, q\ks, [\ptr, #\off]
    pmull v\t1\().1q, v\a\().1d, v\k\().1d
    pmull2 v\t2\().1q, v\a\().2d, v\k\().2d
    pmull v\t3\().1q, v\a\().1d, v\ks\().1d
    pmull2 v\t4\().1q, v\a\().2d, v\ks\().2d
    eor v\lo\().16b, v\lo\().16b, v\t1\().16b
    eor v\hi\().16b, v\hi\().16b, v\t2\().16b
    rj_x3 \mid, \mid, \t3, \t4
.endm
",
            // tail: (P + h0) R + z0 R2 + sum_k z_k (x^k R2) + L T + S, as independent
            // products into two accumulator sets and one reduction. r points to R, R2
            // (each with its half-swapped copy), x to x^1..x^5 R2 likewise, tk to T.
            // L T: L < 2^64 in lane 0, so only the low products.
            r"
.macro rj_fold_final r, x, tk, len, s0, s1, plo, phi, t
    rj_x3 9, 0, 1, 2
    rj_x3 9, 9, 3, 4
    rj_x3 9, 9, 5, 6
    eor v9.16b, v9.16b, v7.16b
    fmov d13, \plo
    mov v13.d[1], \phi
    eor v9.16b, v9.16b, v13.16b
    rj_x3 10, 1, 3, 5
    rj_x3 10, 10, 7, 28
    rj_x3 11, 2, 3, 6
    eor v11.16b, v11.16b, v7.16b
    rj_x3 12, 4, 5, 6
    eor v12.16b, v12.16b, v7.16b
.irp i, 13, 14, 15, 16, 17, 18
    rj_zero \i
.endr
    rj_term 9, \r, 0, 19, 20, 21, 22, 23, 24, 13, 14, 15
    rj_term 10, \r, 32, 0, 1, 2, 3, 4, 5, 16, 17, 18
    rj_term 11, \x, 0, 19, 20, 21, 22, 23, 24, 13, 14, 15
    rj_term 12, \x, 32, 0, 1, 2, 3, 4, 5, 16, 17, 18
    rj_term 29, \x, 64, 19, 20, 21, 22, 23, 24, 13, 14, 15
    rj_term 30, \x, 96, 0, 1, 2, 3, 4, 5, 16, 17, 18
    rj_term 31, \x, 128, 19, 20, 21, 22, 23, 24, 13, 14, 15
    fmov d6, \len
    ldp q7, q8, [\tk]
    pmull v25.1q, v6.1d, v7.1d
    pmull v26.1q, v6.1d, v8.1d
    rj_x3 13, 13, 16, 25
    eor v14.16b, v14.16b, v17.16b
    rj_x3 15, 15, 18, 26
    movi v27.16b, #0
    mov \t, #0x87
    dup v28.2d, \t
    ext v19.16b, v27.16b, v15.16b, #8
    ext v20.16b, v15.16b, v27.16b, #8
    eor v13.16b, v13.16b, v19.16b
    eor v14.16b, v14.16b, v20.16b
    pmull2 v19.1q, v14.2d, v28.2d
    ext v20.16b, v19.16b, v27.16b, #8
    ext v19.16b, v27.16b, v19.16b, #8
    eor v14.16b, v14.16b, v20.16b
    eor v13.16b, v13.16b, v19.16b
    pmull v19.1q, v14.1d, v28.1d
    fmov d20, \s0
    mov v20.d[1], \s1
    rj_x3 13, 13, 19, 20
    fmov \plo, d13
    mov \phi, v13.d[1]
.endm
",
        )
    };
    (purge) => {
        ".purgem rj_zero\n.purgem rj_tail_end\n.purgem rj_tail_case\n.purgem rj_term\n.purgem rj_fold_final"
    };
}

/// An `asm!` block with the macros defined around `body` and every vector
/// register clobbered.
///
/// LLVM sizes an `asm!` block for inlining and branch relaxation by counting
/// its lines (4 bytes each), not by assembling it. A macro call is one line
/// but many instructions, so each kernel states the rest of its size as
/// `pad` bytes: `.space` inside `.if 0` counts but emits nothing. The pads
/// make each estimate equal to that of the fully expanded listing, so the
/// compiler lays out the surrounding code exactly as for that listing; too
/// small a pad can fail the build ("fixup value out of range").
macro_rules! kernel_asm {
    ($x3:ident, [$($set:ident),*], $pad:literal, [$($body:tt)*], [$($operands:tt)*], options($($opt:ident),*)) => {
        core::arch::asm!(
            $x3!(def),
            $($set!(def),)*
            $($body)*,
            concat!(".if 0\n.space ", $pad, "\n.endif"),
            $($set!(purge),)*
            $x3!(purge),
            $($operands)*
            out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
            out("v6") _, out("v7") _, out("v8") _, out("v9") _, out("v10") _, out("v11") _,
            out("v12") _, out("v13") _, out("v14") _, out("v15") _, out("v16") _, out("v17") _,
            out("v18") _, out("v19") _, out("v20") _, out("v21") _, out("v22") _, out("v23") _,
            out("v24") _, out("v25") _, out("v26") _, out("v27") _, out("v28") _, out("v29") _,
            out("v30") _, out("v31") _,
            options($($opt),*),
        )
    };
}

macro_rules! bulk {
    ($name:ident, $features:literal, $x3:ident, $pad:literal) => {
        /// Absorb and close `chunks` (at least one) full chunks at `data`, updating
        /// the outer accumulator `acc`. `keys` points to R, R with halves swapped,
        /// R2, R2 with halves swapped. The chunk fold and outer step run inside the
        /// loop, overlapping the next chunk's loads; the loop performs no stores
        /// (a store whose page offset the following loads approach stalls them).
        ///
        /// # Safety
        /// `data` must be readable for `8192 * chunks` bytes, `table` for 8320
        #[doc = concat!("bytes, `keys` for four words, and the CPU must support `", $features, "`.")]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, chunks: usize, keys: *const u128, acc: u128) -> u128 {
            let (mut plo, mut phi) = (acc as u64, (acc >> 64) as u64);
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_core, rj_fold, rj_one],
                    $pad,
                    [r"
4:
    mov {k}, {k0}
.irp r, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 29, 30, 31
    movi v\r\().16b, #0
.endr
    mov {u}, #0
2:
    rj_group {d}, {k}
    rj_planes {u}
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    cmp {u}, #8
    b.ne 2b
    rj_end_a {k}, 0
    rj_fold_one {r}, {plo}, {phi}, {t}
    subs {n}, {n}, #1
    b.ne 4b
"],
                    [                    d = inout(reg) data => _,
                    k0 = in(reg) table,
                    k = out(reg) _,
                    n = inout(reg) chunks => _,
                    r = in(reg) keys,
                    plo = inout(reg) plo,
                    phi = inout(reg) phi,
                    u = out(reg) _,
                    t = out(reg) _,],
                    options(nostack, readonly)
                );
            }
            plo as u128 | (phi as u128) << 64
        }
    };
}

// Two-chunk kernel. Key rows are reused by every chunk, so chunks A and B
// (the next 8 KiB) are hashed together and each key load serves both: half
// the key bytes per data byte, which is what limits throughput from L2.
//
// Registers are too few for two chunks' full rows, so each group of eight
// positions runs as four passes, one per 16-byte lane pair q (X bytes
// 16q..16q+16 and the matching Y bytes). A pass starts from the previous
// block's lane pair, reloaded from memory (zero before the first block). The
// group sum C is not kept: with T = XOR of all B[v] after group u,
// F0 = T0+..+T7, F1 = T1+T3+T5+T7 and F2 = T3+T7 (differences of consecutive
// T telescope). The end position E is added to B[0] and B[1]: it cancels in
// h0 and enters z0, as its column (0, 1) requires.
//
//   v0..v7   B[v] of A     v8..v15  B[v] of B     v16..v18 F of A
//   v19..v21 F of B        v22..v25 A rows (two sets of x, y)
//   v26..v29 B rows        v30      key word      v31      product / T
macro_rules! bulk2 {
    ($name:ident, $features:literal, $x3:ident, $pad:literal) => {
        /// Absorb and close `pairs` (at least one) pairs of full chunks at `data`,
        /// updating the outer accumulator `acc`. Both chunks of a pair share each key
        /// load, halving key traffic; outputs equal two `bulk` steps. `keys` points
        /// to R, R with halves swapped, R2, R2 with halves swapped; `zero` to 128
        /// zero bytes. The loop performs no stores.
        ///
        /// # Safety
        /// `data` must be readable for `16384 * pairs` bytes, `table` for 8320
        /// bytes, `keys` for four words, `zero` for 128 bytes, and the CPU must
        #[doc = concat!("support `", $features, "`.")]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, pairs: usize, keys: *const u128, zero: *const u8, acc: u128) -> u128 {
            let (mut plo, mut phi) = (acc as u64, (acc >> 64) as u64);
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_fold, rj_mix, rj_pair],
                    $pad,
                    [r"
4:
    mov {k}, {k0}
.irp r, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21
    movi v\r\().16b, #0
.endr
    mov {u}, #0
2:
    rj_pgroup {d}, {k}, {u}, {z}, {t}, {pa}, {pb}
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    add {u}, {u}, #1
    cmp {u}, #8
    b.ne 2b
    rj_pend {d}, {k}
    rj_bsums 0, 1, 2, 3, 4, 5, 6, 7, 22, 23, 24, 25
    rj_h1 {t}, 23, 24, 25, 16, 17, 18, 0, 1, 2, 3, 4, 5, 6, 7, 26, 27, 28, 29, 0, 1
    rj_outer {r}, {plo}, {phi}, 22, 23, 2, 3, 4, 6, 7, 24, 25, 0, 1, 5, 26, 27, 2, 28, 29
    rj_bsums 8, 9, 10, 11, 12, 13, 14, 15, 22, 23, 24, 25
    rj_h1 {t}, 23, 24, 25, 19, 20, 21, 8, 9, 10, 11, 12, 13, 14, 15, 26, 27, 28, 29, 8, 9
    rj_outer {r}, {plo}, {phi}, 22, 23, 10, 11, 12, 14, 15, 24, 25, 8, 9, 13, 26, 27, 10, 28, 29
    add {d}, {d}, #8192
    subs {n}, {n}, #1
    b.ne 4b
"],
                    [                    d = inout(reg) data => _,
                    k0 = in(reg) table,
                    k = out(reg) _,
                    n = inout(reg) pairs => _,
                    r = in(reg) keys,
                    z = in(reg) zero,
                    pa = out(reg) _,
                    pb = out(reg) _,
                    plo = inout(reg) plo,
                    phi = inout(reg) phi,
                    u = out(reg) _,
                    t = out(reg) _,],
                    options(nostack, readonly)
                );
            }
            plo as u128 | (phi as u128) << 64
        }
    };
}

// Two-chunk kernel with two lane pairs per pass. A pass over the adjacent
// lane pairs (2p, 2p + 1) touches four of a line's eight 16-byte L1 banks
// and loads each chunk's X and Y words with one LDP each; one lane pair per
// pass touches two banks and loses load throughput to bank conflicts. The rows
// then need 16 registers, so each chunk keeps seven accumulators instead of
// B[0..8]: E0 and E1 (products at even and odd v), P1 and P2 (positions whose
// v has bit 1 or 2 set) and the planes F0..F2 kept as above. Then
// h0 = E0 + E1, z0 = E1, z1 = P1, z2 = P2, and the end position adds E to
// both E0 and E1.
//
//   v0..v6   A: E0 E1 P1 P2 F0 F1 F2     v7..v13  B: the same
//   v14..v21 A rows: two sets of (x_2p, y_2p, x_2p+1, y_2p+1)
//   v22..v29 B rows                     v30 key word    v31 product / T
//
// With prefetch (`pf`), lines 32 KiB ahead are prefetched into L2: far enough
// to cover DRAM latency, measured best at 24–48 KiB on M1.
macro_rules! bulk4 {
    ($name:ident, $features:literal, $x3:ident, $pad:literal, $group:literal, $pfdoc:literal $(, $f:ident)?) => {
        /// Absorb and close `pairs` (at least one) pairs of full chunks at `data`,
        /// updating the outer accumulator `acc`; outputs equal two `bulk` steps. Both
        /// chunks of a pair share each key load, and each pass reads two lane pairs,
        #[doc = concat!("so loads spread over four L1 banks. ", $pfdoc, "`keys` points to R, R with")]
        /// halves swapped, R2, R2 with halves swapped; `zero` to 128 zero bytes. The
        /// loop performs no stores.
        ///
        /// # Safety
        /// `data` must be readable for `16384 * pairs` bytes, `table` for 8320
        /// bytes, `keys` for four words, `zero` for 128 bytes, and the CPU must
        #[doc = concat!("support `", $features, "`.")]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, pairs: usize, keys: *const u128, zero: *const u8, acc: u128) -> u128 {
            let (mut plo, mut phi) = (acc as u64, (acc >> 64) as u64);
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_fold, rj_mix, rj_quad],
                    $pad,
                    [r"
4:
    mov {k}, {k0}
.irp r, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13
    movi v\r\().16b, #0
.endr
    mov {u}, #0
2:
", $group, r"
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    add {u}, {u}, #1
    cmp {u}, #8
    b.ne 2b
    add {t}, {d}, #8192
    sub {t}, {t}, #128
    rj_qend {d}, {k}, {t}
    rj_qfold {r}, {plo}, {phi}, {t}, 0, 1, 2, 3, 4, 5, 6
    rj_qfold {r}, {plo}, {phi}, {t}, 7, 8, 9, 10, 11, 12, 13
    add {d}, {d}, #8192
    subs {n}, {n}, #1
    b.ne 4b
"],
                    [                    d = inout(reg) data => _,
                    k0 = in(reg) table,
                    k = out(reg) _,
                    n = inout(reg) pairs => _,
                    r = in(reg) keys,
                    z = in(reg) zero,
                    pa = out(reg) _,
                    pb = out(reg) _,
                    db = out(reg) _,
                    plo = inout(reg) plo,
                    phi = inout(reg) phi,
                    u = out(reg) _,
                    t = out(reg) _,
                    $($f = out(reg) _,)?],
                    options(nostack, readonly)
                );
            }
            plo as u128 | (phi as u128) << 64
        }
    };
}

macro_rules! tail {
    ($name:ident, $features:literal, $x3:ident, $pad:literal) => {
        /// The hash of a message ending in a partial chunk: `groups` whole groups
        /// and `blocks` (< 8) whole blocks at `data`, then the zero-padded final
        /// block at `last` if it is not null, then the end position; `acc` is the
        /// outer accumulator of the whole chunks before (zero if none). Returns
        /// `(acc + h0) R + h1 R2 + len T + S`. The chunk must have fewer than 64
        /// blocks, or exactly 64 with the last one at `last`. `keys` points to R, R2
        /// (each followed by its half-swapped copy), `r2x` to `x^1..=x^5 R2` and `t`
        /// to T likewise. All state stays in registers.
        ///
        /// # Safety
        /// `data` must be readable for its whole blocks, `last` (if not null) for
        /// 128 bytes, `table` for the rows used, the key pointers as above, and the
        #[doc = concat!("CPU must support `", $features, "`.")]
        #[allow(clippy::too_many_arguments)]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, groups: usize, blocks: usize, last: *const u8, keys: *const u128, r2x: *const u128, t: *const u128, s: u128, len: u64, acc: u128) -> u128 {
            let (mut plo, mut phi) = (acc as u64, (acc >> 64) as u64);
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_core, rj_tail],
                    $pad,
                    [r"
.irp i, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 29, 30, 31
    movi v\i\().16b, #0
.endr
    mov {u}, #0
    cbz {g}, 5f
2:
    rj_group {d}, {k}
    rj_planes {u}
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    subs {g}, {g}, #1
    b.ne 2b
5:
.irp v, 0, 1, 2, 3, 4, 5, 6
    cmp {kb}, #\v
    b.eq 7\v\()f
    rj_block {d}, {k}, (128 * \v), \v
.endr
    b 77f
    rj_tail_case {k}, {u}, {last}, 0, 1
    rj_tail_case {k}, {u}, {last}, 1, 2
    rj_tail_case {k}, {u}, {last}, 2, 3
    rj_tail_case {k}, {u}, {last}, 3, 4
    rj_tail_case {k}, {u}, {last}, 4, 5
    rj_tail_case {k}, {u}, {last}, 5, 6
    rj_tail_case {k}, {u}, {last}, 6, 7
    rj_tail_case {k}, {u}, {last}, 7, 8
9:
    rj_fold_final {r}, {x}, {tk}, {len}, {s0}, {s1}, {plo}, {phi}, {t}
"],
                    [                    d = inout(reg) data => _,
                    k = inout(reg) table => _,
                    g = inout(reg) groups => _,
                    kb = in(reg) blocks,
                    last = in(reg) last,
                    r = in(reg) keys,
                    x = in(reg) r2x,
                    tk = in(reg) t,
                    s0 = in(reg) s as u64,
                    s1 = in(reg) (s >> 64) as u64,
                    len = in(reg) len,
                    plo = inout(reg) plo,
                    phi = inout(reg) phi,
                    u = out(reg) _,
                    t = out(reg) _,],
                    options(nostack, readonly)
                );
            }
            plo as u128 | (phi as u128) << 64
        }
    };
}

macro_rules! groups {
    ($name:ident, $features:literal, $x3:ident, $pad:literal) => {
        /// Absorb `groups` (at least one) groups of eight blocks at `data`, the first
        /// being group `u` of its chunk, keyed by the table rows starting at `table`.
        ///
        /// `state` holds B[0..8] at words 0..8, F[0..3] at words 8..11 and the
        /// previous block's rows at words 16..24, read and updated.
        ///
        /// # Safety
        /// `data` must be readable for `1024 * groups` bytes, `table` for as many
        #[doc = concat!("key bytes, `u + groups <= 8`, and the CPU must support `", $features, "`.")]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, groups: usize, u: usize, state: &mut [u128; 24]) {
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_core],
                    $pad,
                    [r"
    ldp q0, q1, [{s}, #0]
    ldp q2, q3, [{s}, #32]
    ldp q4, q5, [{s}, #64]
    ldp q6, q7, [{s}, #96]
    ldp q29, q30, [{s}, #128]
    ldr q31, [{s}, #160]
    ldp q9, q10, [{s}, #256]
    ldp q11, q12, [{s}, #288]
    ldp q13, q14, [{s}, #320]
    ldp q15, q16, [{s}, #352]
    movi v8.16b, #0
2:
    rj_group {d}, {k}
    rj_planes {u}
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    subs {g}, {g}, #1
    b.ne 2b
    stp q0, q1, [{s}, #0]
    stp q2, q3, [{s}, #32]
    stp q4, q5, [{s}, #64]
    stp q6, q7, [{s}, #96]
    stp q29, q30, [{s}, #128]
    str q31, [{s}, #160]
    stp q9, q10, [{s}, #256]
    stp q11, q12, [{s}, #288]
    stp q13, q14, [{s}, #320]
    stp q15, q16, [{s}, #352]
"],
                    [                    d = inout(reg) data => _,
                    k = inout(reg) table => _,
                    s = in(reg) state.as_mut_ptr(),
                    g = inout(reg) groups => _,
                    u = inout(reg) u => _,],
                    options(nostack)
                );
            }
        }
    };
}

macro_rules! groups0 {
    ($name:ident, $features:literal, $x3:ident, $pad:literal) => {
        /// Absorb `groups` (at least one) groups of eight blocks at `data`, the first
        /// being group `u` of its chunk, keyed by the table rows starting at `table`.
        ///
        /// The kernel starts from zero and writes B[0..8] to words 0..8 of `state`,
        /// F[0..3] to words 8..11 and the last block's rows to words 16..24. It
        /// never reads `state`, which may be uninitialized.
        ///
        /// # Safety
        /// `data` must be readable for `1024 * groups` bytes, `table` for as many
        /// key bytes, `state` writable for 24 words, `u + groups <= 8`, and the CPU
        #[doc = concat!("must support `", $features, "`.")]
        #[target_feature(enable = $features)]
        pub unsafe fn $name(data: *const u8, table: *const u8, groups: usize, u: usize, state: *mut u128) {
            // A fresh state is output-only and may be uninitialized, so it is
            // passed as a raw pointer: a reference would assert initialized memory.
            unsafe {
                kernel_asm!(
                    $x3,
                    [rj_core],
                    $pad,
                    [r"
.irp i, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 29, 30, 31
    movi v\i\().16b, #0
.endr
    movi v8.16b, #0
2:
    rj_group {d}, {k}
    rj_planes {u}
    add {d}, {d}, #1024
    add {k}, {k}, #1024
    subs {g}, {g}, #1
    b.ne 2b
    stp q0, q1, [{s}, #0]
    stp q2, q3, [{s}, #32]
    stp q4, q5, [{s}, #64]
    stp q6, q7, [{s}, #96]
    stp q29, q30, [{s}, #128]
    str q31, [{s}, #160]
    stp q9, q10, [{s}, #256]
    stp q11, q12, [{s}, #288]
    stp q13, q14, [{s}, #320]
    stp q15, q16, [{s}, #352]
"],
                    [                    d = inout(reg) data => _,
                    k = inout(reg) table => _,
                    s = in(reg) state,
                    g = inout(reg) groups => _,
                    u = inout(reg) u => _,],
                    options(nostack)
                );
            }
        }
    };
}

bulk!(bulk_eor3, "neon,aes,sha3", x3_eor3, 860);
bulk!(bulk_plain, "neon,aes", x3_plain, 1232);
bulk2!(bulk2_eor3, "neon,aes,sha3", x3_eor3, 2360);
bulk2!(bulk2_plain, "neon,aes", x3_plain, 3268);
// The prefetching variants pass the prefetch base register; the others pass
// `xzr`, which `rj_qgroup` never reads when its prefetch flag is 0.
bulk4!(bulk4_eor3, "neon,aes,sha3", x3_eor3, 2060,
    "    rj_qgroup {d}, {db}, {k}, xzr, 0, {u}, {z}, {t}, {pa}, {pb}", "");
bulk4!(bulk4_plain, "neon,aes", x3_plain, 2752,
    "    rj_qgroup {d}, {db}, {k}, xzr, 0, {u}, {z}, {t}, {pa}, {pb}", "");
bulk4!(bulk4pf_eor3, "neon,aes,sha3", x3_eor3, 2128,
    "    rj_qgroup {d}, {db}, {k}, {f}, 1, {u}, {z}, {t}, {pa}, {pb}",
    "Lines 32 KiB ahead are prefetched into L2, for inputs that stream from memory. ", f);
bulk4!(bulk4pf_plain, "neon,aes", x3_plain, 2820,
    "    rj_qgroup {d}, {db}, {k}, {f}, 1, {u}, {z}, {t}, {pa}, {pb}",
    "Lines 32 KiB ahead are prefetched into L2, for inputs that stream from memory. ", f);
tail!(tail_eor3, "neon,aes,sha3", x3_eor3, 5736);
tail!(tail_plain, "neon,aes", x3_plain, 6964);
groups!(groups_eor3, "neon,aes,sha3", x3_eor3, 732);
groups!(groups_plain, "neon,aes", x3_plain, 1040);
groups0!(groups0_eor3, "neon,aes,sha3", x3_eor3, 832);
groups0!(groups0_plain, "neon,aes", x3_plain, 1140);

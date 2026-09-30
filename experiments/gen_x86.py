#!/usr/bin/env python3
"""Generate x86-64 AVX-512 inline-asm bodies for the carryless + Multimixer
hybrid (CANDIDATES.md §9.3): `src/x86_asm.rs`.

`hyb<N>` absorbs the 64 carryless blocks and 64/N Multimixer tiles of one
chunk laid out as super-groups [N blocks][one 512-byte tile], in the program
order the generator chooses (the point of the exercise: LLVM bunches the
carryless multiplies, which stalls dispatch on Zen 5). It starts from zero
accumulators and stores, to `out`: the block sums b[0..8] (by position mod 8),
the Multimixer sums z[0..8], and the last block's rows (px, py); and to `ts`
the running total of b after each 8 blocks. The chunk close is Rust.

Registers: b = zmm0-7, z = zmm8-15, carryless zmm16-21, tile zmm22-31.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))


PF = 0  # prefetch distance in bytes (0: none)


def block(j, d_off, k_off, pair):
    """Carryless block j: rows at [d + d_off], key row at [k + k_off].
    `pair` is 0 or 1: which of (zmm16,17)/(zmm18,19) holds the previous rows."""
    px, py = (16, 17) if pair == 0 else (18, 19)
    nx, ny = (18, 19) if pair == 0 else (16, 17)
    b = j % 8
    pf = [f"prefetcht0 [{{d}} + {d_off + PF}]", f"prefetcht0 [{{d}} + {d_off + 64 + PF}]"] if PF else []
    return pf + [
        f"vmovdqu64 zmm{nx}, zmmword ptr [{{d}} + {d_off}]",
        f"vmovdqu64 zmm{ny}, zmmword ptr [{{d}} + {d_off + 64}]",
        f"vpternlogq zmm{px}, zmm{nx}, zmmword ptr [{{k}} + {k_off}], 0x96",
        f"vpternlogq zmm{py}, zmm{ny}, zmmword ptr [{{k}} + {k_off + 64}], 0x96",
        f"vpclmulqdq zmm20, zmm{px}, zmm{py}, 0x00",
        f"vpclmulqdq zmm21, zmm{px}, zmm{py}, 0x11",
        f"vpternlogq zmm{b}, zmm20, zmm21, 0x96",
    ]


def total(ts_off):
    """Running total of b0..b7 stored at [ts + ts_off] (zmm20/21 as temps)."""
    return [
        "vpxorq zmm20, zmm0, zmm1",
        "vpternlogq zmm20, zmm2, zmm3, 0x96",
        "vpxorq zmm21, zmm4, zmm5",
        "vpternlogq zmm21, zmm6, zmm7, 0x96",
        "vpxorq zmm20, zmm20, zmm21",
        f"vmovdqu64 zmmword ptr [{{ts}} + {ts_off}], zmm20",
    ]


def tile(d_off, t_off, x_off):
    """Multimixer on the transposed tile at [d + d_off], keys at
    [kt + t_off], derived key sums (k0+..+k3, k4+..+k7) at [kx + x_off]."""
    M = [f"zmm{22 + i}" for i in range(10)]
    sx, sy, a3, ap, ac, bc, u, v, t1, t2 = M
    D = lambda c: f"zmmword ptr [{{d}} + {d_off + 64 * c}]"
    K = lambda c: f"zmmword ptr [{{kt}} + {t_off + 64 * c}]"
    out = [f"prefetcht0 [{{d}} + {d_off + 64 * c + PF}]" for c in range(8)] if PF else []
    out += [
        f"vmovdqu64 {sx}, {D(0)}",
        f"vpaddd {sx}, {sx}, {D(1)}",
        f"vpaddd {sx}, {sx}, {D(2)}",
        f"vpaddd {sx}, {sx}, {D(3)}",
        f"vpaddd {sx}, {sx}, zmmword ptr [{{kx}} + {x_off}]",
        f"vmovdqu64 {sy}, {D(4)}",
        f"vpaddd {sy}, {sy}, {D(5)}",
        f"vpaddd {sy}, {sy}, {D(6)}",
        f"vpaddd {sy}, {sy}, {D(7)}",
        f"vpaddd {sy}, {sy}, zmmword ptr [{{kx}} + {x_off + 64}]",
        f"vmovdqu64 {a3}, {K(3)}",
        f"vpaddd {a3}, {a3}, {D(3)}",
    ]

    def prod(a, b, z, keep_a):
        # z += lo(a) lo(b) + hi(a) hi(b), per 64-bit lane.
        r = [
            f"vpmuludq {t1}, {a}, {b}",
            f"vpaddq zmm{z}, zmm{z}, {t1}",
            f"vpsrlq {t1}, {a}, 32",
            f"vpsrlq {t2}, {b}, 32",
            f"vpmuludq {t1}, {t1}, {t2}",
            f"vpaddq zmm{z}, zmm{z}, {t1}",
        ]
        return r

    prev, cur = ap, ac
    for i in range(4):
        a = a3 if i == 3 else cur
        if i < 3:
            out += [f"vmovdqu64 {cur}, {K(i)}", f"vpaddd {cur}, {cur}, {D(i)}"]
        out += [f"vmovdqu64 {bc}, {K(4 + i)}", f"vpaddd {bc}, {bc}, {D(4 + i)}"]
        # u_i = sx - a_{i+3}: a3 for i = 0, else the previous a.
        out.append(f"vpsubd {u}, {sx}, {a3 if i == 0 else prev}")
        out.append(f"vpsubd {v}, {sy}, {bc}")
        out += prod(a, bc, 8 + i, True)
        out += prod(u, v, 12 + i, False)
        if i < 3:
            prev, cur = cur, prev
    return out


def body(n, spread):
    """One loop iteration: max(n, 8) blocks and max(n, 8) // n tiles."""
    nb = max(n, 8)
    nt = nb // n
    group = 128 * n + 512
    blocks = []
    tiles = []
    for s in range(nt):
        base = s * group
        for v in range(n):
            j = s * n + v
            blocks.append(block(j, base + 128 * v, 128 * j, j % 2))
            if j % 8 == 7:
                blocks[-1] = blocks[-1] + total(64 * (j // 8))
        tiles += tile(base + 128 * n, 512 * s, 128 * s)
    lines = []
    if not spread:
        # Program order as written in Rust: each super-group's blocks, then
        # its tile.
        per = len(tiles) // nt
        for s in range(nt):
            for v in range(n):
                lines += blocks[s * n + v]
            lines += tiles[s * per:(s + 1) * per]
        return lines, nb, nt, group * nt
    # Spread the tile instructions evenly after each block.
    emitted = 0
    for i, blk in enumerate(blocks):
        lines += blk
        upto = round((i + 1) * len(tiles) / nb)
        lines += tiles[emitted:upto]
        emitted = upto
    return lines, nb, nt, group * nt


def kernel(n, spread):
    lines, nb, nt, step = body(n, spread)
    name = f"hyb{n}" + ("" if spread else "_seq")
    iters = 64 // nb
    out = []
    out.append(f"/// {nb} carryless blocks and {nt} tile(s) per iteration, {iters} iterations;")
    out.append(f"/// {'tile instructions spread between blocks' if spread else 'blocks then tile, as in the Rust source'}.")
    out.append("#[target_feature(enable = \"avx512f,avx512bw,avx512vl,vpclmulqdq\")]")
    out.append(f"pub unsafe fn {name}(d: *const u8, k: *const u8, kt: *const u8, kx: *const u8, ts: *mut u8, out: *mut u8) {{")
    out.append("    unsafe {")
    out.append("        core::arch::asm!(")
    pre = [f"vpxord zmm{i}, zmm{i}, zmm{i}" for i in list(range(16)) + [16, 17]]
    loop = ["2:"] + lines + [
        f"add {{d}}, {step}",
        f"add {{k}}, {128 * nb}",
        f"add {{kt}}, {512 * nt}",
        f"add {{kx}}, {128 * nt}",
        f"add {{ts}}, {64 * (nb // 8)}",
        "dec {n}",
        "jnz 2b",
    ]
    # After an even number of blocks the previous rows are in zmm16/17.
    post = [f"vmovdqu64 zmmword ptr [{{out}} + {64 * i}], zmm{i}" for i in range(16)]
    post += ["vmovdqu64 zmmword ptr [{out} + 1024], zmm16", "vmovdqu64 zmmword ptr [{out} + 1088], zmm17"]
    for l in pre + loop + post:
        out.append(f"            \"{l}\",")
    out.append("            d = inout(reg) d => _,")
    out.append("            k = inout(reg) k => _,")
    out.append("            kt = inout(reg) kt => _,")
    out.append("            kx = inout(reg) kx => _,")
    out.append("            ts = inout(reg) ts => _,")
    out.append("            out = in(reg) out,")
    out.append(f"            n = inout(reg) {iters}usize => _,")
    for i in range(32):
        out.append(f"            out(\"zmm{i}\") _,")
    out.append("            options(nostack),")
    out.append("        );")
    out.append("    }")
    out.append("}")
    return "\n".join(out)


def close_lines():
    """The chunk close, in asm, after the body loop of a whole chunk (64
    carryless blocks): endpoint, planes, `h1`, the Multimixer lane sums, the
    six-coordinate joint outer product (keys `(R, A1, A2, A3)` at [sc + 512],
    `(A4, A5, 0, 0)` at [sc + 576]), shift reduction and lane fold, with `P`
    read from and written to [sc + 640]. Group totals are at [sc + 64 u]."""
    L = []
    e = L.extend
    # Endpoint (row 64) with the last rows in zmm16/17.
    e([
        "vpxorq zmm18, zmm16, zmmword ptr [{k0} + 8192]",
        "vpxorq zmm19, zmm17, zmmword ptr [{k0} + 8256]",
        "vpclmulqdq zmm20, zmm18, zmm19, 0x00",
        "vpclmulqdq zmm21, zmm18, zmm19, 0x11",
        "vpxorq zmm20, zmm20, zmm21",
    ])
    # h0 = b0 + .. + b7 (zmm21); planes z0 (zmm20), z1 (zmm22), z2 (zmm23).
    e([
        "vpxorq zmm21, zmm0, zmm1",
        "vpternlogq zmm21, zmm2, zmm3, 0x96",
        "vpternlogq zmm21, zmm4, zmm5, 0x96",
        "vpternlogq zmm21, zmm6, zmm7, 0x96",
        "vpternlogq zmm20, zmm1, zmm3, 0x96",
        "vpternlogq zmm20, zmm5, zmm7, 0x96",
        "vpxorq zmm22, zmm2, zmm3",
        "vpternlogq zmm22, zmm6, zmm7, 0x96",
        "vpxorq zmm23, zmm4, zmm5",
        "vpternlogq zmm23, zmm6, zmm7, 0x96",
    ])
    # From the group totals T0..T7: F1 = odd ones (zmm24), F2 = T3 + T7
    # (zmm25), F0 = all (zmm26).
    e([
        "vmovdqu64 zmm24, zmmword ptr [{sc} + 64]",
        "vmovdqu64 zmm25, zmmword ptr [{sc} + 192]",
        "vpternlogq zmm24, zmm25, zmmword ptr [{sc} + 320], 0x96",
        "vpxorq zmm24, zmm24, zmmword ptr [{sc} + 448]",
        "vpxorq zmm25, zmm25, zmmword ptr [{sc} + 448]",
        "vmovdqu64 zmm26, zmmword ptr [{sc}]",
        "vpternlogq zmm26, zmm24, zmmword ptr [{sc} + 128], 0x96",
        "vmovdqu64 zmm27, zmmword ptr [{sc} + 256]",
        "vpternlogq zmm26, zmm27, zmmword ptr [{sc} + 384], 0x96",
    ])
    # h1 = sum_k x^k z_k reduced (zmm18); z = (20, 22, 23, 26, 24, 25).
    def shl(dst, src, k):
        return [f"vpslldq {dst}, {src}, 8", f"vpshldq {dst}, {src}, {dst}, {k}"]
    e(shl("zmm18", "zmm22", 1) + shl("zmm19", "zmm23", 2))
    e(["vpternlogq zmm18, zmm19, zmm20, 0x96"])
    e(shl("zmm19", "zmm26", 3) + shl("zmm27", "zmm24", 4))
    e(["vpternlogq zmm18, zmm19, zmm27, 0x96"])
    e(shl("zmm19", "zmm25", 5))
    e(["vpxorq zmm18, zmm18, zmm19"])
    e([
        "vpsrlq zmm19, zmm22, 63",
        "vpsrlq zmm27, zmm23, 62",
        "vpsrlq zmm28, zmm26, 61",
        "vpternlogq zmm19, zmm27, zmm28, 0x96",
        "vpsrlq zmm27, zmm24, 60",
        "vpsrlq zmm28, zmm25, 59",
        "vpternlogq zmm19, zmm27, zmm28, 0x96",
        "vpsrldq zmm19, zmm19, 8",
        "vpsllq zmm27, zmm19, 1",
        "vpsllq zmm28, zmm19, 2",
        "vpternlogq zmm27, zmm28, zmm19, 0x96",
        "vpsllq zmm28, zmm19, 7",
        "vpternlogq zmm18, zmm27, zmm28, 0x96",
    ])
    # fold2(h0, h1) = (h0, h0, h1, h1) lane sums; P into lane 0; then
    # (h0 + P, h1, ..) (zmm22).
    e([
        "vshufi64x2 zmm22, zmm21, zmm18, 0x44",
        "vshufi64x2 zmm23, zmm21, zmm18, 0xEE",
        "vpxorq zmm22, zmm22, zmm23",
        "vshufi64x2 zmm23, zmm22, zmm22, 0xB1",
        "vpxorq zmm22, zmm22, zmm23",
        "vmovdqu64 xmm23, xmmword ptr [{sc} + 640]",
        "vpxorq zmm22, zmm22, zmm23",
        "vshufi64x2 zmm22, zmm22, zmm22, 0x08",
    ])
    # Multimixer lane sums packed (z0|z1, z2|z3, z4|z5, z6|z7) (zmm29).
    for (a, b, t, u) in ((8, 9, 23, 24), (10, 11, 24, 25), (12, 13, 25, 26), (14, 15, 26, 27)):
        pass
    e([
        "vpunpcklqdq zmm23, zmm8, zmm9", "vpunpckhqdq zmm24, zmm8, zmm9", "vpaddq zmm23, zmm23, zmm24",
        "vpunpcklqdq zmm24, zmm10, zmm11", "vpunpckhqdq zmm25, zmm10, zmm11", "vpaddq zmm24, zmm24, zmm25",
        "vpunpcklqdq zmm25, zmm12, zmm13", "vpunpckhqdq zmm26, zmm12, zmm13", "vpaddq zmm25, zmm25, zmm26",
        "vpunpcklqdq zmm26, zmm14, zmm15", "vpunpckhqdq zmm27, zmm14, zmm15", "vpaddq zmm26, zmm26, zmm27",
        "vshufi64x2 zmm27, zmm23, zmm24, 0x88", "vshufi64x2 zmm28, zmm23, zmm24, 0xDD", "vpaddq zmm27, zmm27, zmm28",
        "vshufi64x2 zmm28, zmm25, zmm26, 0x88", "vshufi64x2 zmm29, zmm25, zmm26, 0xDD", "vpaddq zmm28, zmm28, zmm29",
        "vshufi64x2 zmm29, zmm27, zmm28, 0x88", "vshufi64x2 zmm30, zmm27, zmm28, 0xDD", "vpaddq zmm29, zmm29, zmm30",
    ])
    # a1 = (h0 + P, h1, W0, W1) (zmm30), a2 = (W2, W3, 0, 0) (zmm31);
    # schoolbook products summed per part.
    e([
        "vshufi64x2 zmm30, zmm22, zmm29, 0x44",
        "vpxorq zmm31, zmm31, zmm31",
        "vshufi64x2 zmm31, zmm29, zmm31, 0x0E",
        "vpclmulqdq zmm0, zmm30, zmmword ptr [{sc} + 512], 0x00",
        "vpclmulqdq zmm1, zmm31, zmmword ptr [{sc} + 576], 0x00",
        "vpclmulqdq zmm2, zmm30, zmmword ptr [{sc} + 512], 0x11",
        "vpclmulqdq zmm3, zmm31, zmmword ptr [{sc} + 576], 0x11",
        "vpclmulqdq zmm4, zmm30, zmmword ptr [{sc} + 512], 0x01",
        "vpclmulqdq zmm5, zmm30, zmmword ptr [{sc} + 512], 0x10",
        "vpclmulqdq zmm6, zmm31, zmmword ptr [{sc} + 576], 0x01",
        "vpclmulqdq zmm7, zmm31, zmmword ptr [{sc} + 576], 0x10",
        "vpxorq zmm0, zmm0, zmm1",
        "vpxorq zmm2, zmm2, zmm3",
        "vpternlogq zmm4, zmm5, zmm6, 0x96",
        "vpxorq zmm4, zmm4, zmm7",
        # lo + mid x^64 (zmm0), hi + mid / x^64 (zmm2).
        "vpslldq zmm1, zmm4, 8",
        "vpxorq zmm0, zmm0, zmm1",
        "vpsrldq zmm1, zmm4, 8",
        "vpxorq zmm2, zmm2, zmm1",
        # Reduce per lane with shifts.
        "vpslldq zmm3, zmm2, 8",
        "vpshldq zmm5, zmm2, zmm3, 1",
        "vpshldq zmm6, zmm2, zmm3, 2",
        "vpshldq zmm7, zmm2, zmm3, 7",
        "vpternlogq zmm5, zmm6, zmm7, 0x96",
        "vpternlogq zmm5, zmm2, zmm0, 0x96",
        "vpsrlq zmm6, zmm2, 63",
        "vpsrlq zmm7, zmm2, 62",
        "vpsrlq zmm3, zmm2, 57",
        "vpternlogq zmm6, zmm7, zmm3, 0x96",
        "vpsrldq zmm6, zmm6, 8",
        "vpsllq zmm7, zmm6, 1",
        "vpsllq zmm3, zmm6, 2",
        "vpternlogq zmm7, zmm3, zmm6, 0x96",
        "vpsllq zmm3, zmm6, 7",
        "vpternlogq zmm5, zmm7, zmm3, 0x96",
        # Sum of the four lanes -> P.
        "vextracti64x4 ymm6, zmm5, 1",
        "vpxorq ymm5, ymm5, ymm6",
        "vextracti128 xmm6, ymm5, 1",
        "vpxorq xmm5, xmm5, xmm6",
        "vmovdqu64 xmmword ptr [{sc} + 640], xmm5",
    ])
    return L


def kernel_full(n, spread=True, pf=0):
    """A whole message of `chunks` chunks: body loop and asm close per chunk,
    everything in registers except the group totals, keys and `P` (scratch
    `sc`)."""
    global PF
    PF = pf
    lines, nb, nt, step = body(n, spread)
    PF = 0
    iters = 64 // nb
    name = f"hybfull{n}" + (f"_pf{pf}" if pf else "")
    lines = [l.replace("{ts}", "{sc}") for l in lines]
    out = []
    out.append(f"/// Whole chunks of the `hyb{n}` layout with the close in asm: `chunks`")
    out.append("/// chunks at `d`; `sc`: 512 bytes of group totals, the outer keys at 512,")
    out.append("/// `P` at 640 (read and written).")
    out.append("#[target_feature(enable = \"avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq\")]")
    out.append(f"pub unsafe fn {name}(d: *const u8, k0: *const u8, kx0: *const u8, sc: *mut u8, chunks: usize) {{")
    out.append("    unsafe {")
    out.append("        core::arch::asm!(")
    loop = ["3:"] + [f"vpxord zmm{i}, zmm{i}, zmm{i}" for i in list(range(16)) + [16, 17]]
    loop += ["mov {k}, {k0}", "lea {kt}, [{k0} + 8320]", "mov {kx}, {kx0}", "mov {sct}, {sc}", f"mov {{n}}, {iters}"]
    inner = ["2:"] + [l.replace("{sc}", "{sct}") for l in lines] + [
        f"add {{d}}, {step}",
        f"add {{k}}, {128 * nb}",
        f"add {{kt}}, {512 * nt}",
        f"add {{kx}}, {128 * nt}",
        f"add {{sct}}, {64 * (nb // 8)}",
        "dec {n}",
        "jnz 2b",
    ]
    loop += inner + close_lines() + ["dec {c}", "jnz 3b"]
    for l in loop:
        out.append(f"            \"{l}\",")
    out.append("            d = inout(reg) d => _,")
    out.append("            k0 = in(reg) k0,")
    out.append("            kx0 = in(reg) kx0,")
    out.append("            sc = in(reg) sc,")
    out.append("            c = inout(reg) chunks => _,")
    out.append("            k = out(reg) _,")
    out.append("            kt = out(reg) _,")
    out.append("            kx = out(reg) _,")
    out.append("            sct = out(reg) _,")
    out.append("            n = out(reg) _,")
    for i in range(32):
        out.append(f"            out(\"zmm{i}\") _,")
    out.append("            options(nostack),")
    out.append("        );")
    out.append("    }")
    out.append("}")
    return "\n".join(out)


def main():
    assert 64 % 2 == 0
    parts = [
        "//! Generated by gen_x86.py; do not edit by hand.",
        "#![allow(clippy::all)]",
        "",
    ]
    for n in (4, 8, 16, 32):
        parts.append(kernel(n, True))
        parts.append("")
    for n in (8, 16):
        parts.append(kernel(n, False))
        parts.append("")
    for n in (8, 16, 32):
        parts.append(kernel_full(n))
        parts.append("")
    for pf in (1024, 2048, 4096):
        parts.append(kernel_full(16, pf=pf))
        parts.append("")
    with open(os.path.join(HERE, "src", "x86_asm.rs"), "w") as f:
        f.write("\n".join(parts))


if __name__ == "__main__":
    main()

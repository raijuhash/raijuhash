#!/usr/bin/env python3
"""Generate AArch64 inline-asm stripe kernels for candidate screening.

Emits `src/asm_kernels.rs`. Each kernel processes one chunk (a fixed number
of stripes) with accumulators held in v0..v{n}; the Rust wrapper loads and
stores the accumulator array around the asm block.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))


class Gen:
    def __init__(self):
        self.lines = []

    def emit(self, s):
        self.lines.append(s)


def chain_stripe(g, n, dreg="{d}", kreg="{k}"):
    """Row-major chain code over n 128-byte blocks; rotating register pool."""
    acc = [f"v{i}" for i in range(n + 1)]
    pool = [f"v{i}" for i in range(n + 1, n + 11)]
    kx, ky, p0, p1 = "v27", "v28", "v29", "v30"
    assert n + 11 <= 27
    prev = {}
    for j in range(n + 1):
        for q in range(4):
            koff = 128 * j + 16 * q
            g.emit(f"ldr q{kx[1:]}, [{kreg}, #{koff}]")
            g.emit(f"ldr q{ky[1:]}, [{kreg}, #{koff + 64}]")
            if j < n:
                doff = 128 * j + 16 * q
                x = pool.pop(0)
                y = pool.pop(0)
                g.emit(f"ldr q{x[1:]}, [{dreg}, #{doff}]")
                g.emit(f"ldr q{y[1:]}, [{dreg}, #{doff + 64}]")
            if j == 0:
                g.emit(f"eor {kx}.16b, {x}.16b, {kx}.16b")
                g.emit(f"eor {ky}.16b, {y}.16b, {ky}.16b")
            elif j < n:
                px, py = prev[q]
                g.emit(f"eor3 {kx}.16b, {px}.16b, {x}.16b, {kx}.16b")
                g.emit(f"eor3 {ky}.16b, {py}.16b, {y}.16b, {ky}.16b")
                pool += [px, py]
            else:
                px, py = prev[q]
                g.emit(f"eor {kx}.16b, {px}.16b, {kx}.16b")
                g.emit(f"eor {ky}.16b, {py}.16b, {ky}.16b")
                pool += [px, py]
            g.emit(f"pmull {p0}.1q, {kx}.1d, {ky}.1d")
            g.emit(f"pmull2 {p1}.1q, {kx}.2d, {ky}.2d")
            g.emit(f"eor3 {acc[j]}.16b, {acc[j]}.16b, {p0}.16b, {p1}.16b")
            if j < n:
                prev[q] = (x, y)


def parity_stripe(g, n, dreg="{d}", kreg="{k}"):
    """Column-major parity code; pairs of blocks fold into parity with EOR3."""
    acc = [f"v{i}" for i in range(n + 1)]
    base = n + 1
    px, py = f"v{base}", f"v{base + 1}"
    x0, y0, x1, y1 = (f"v{base + 2 + i}" for i in range(4))
    k0, k1, k2, k3 = (f"v{base + 6 + i}" for i in range(4))
    p0, p1 = f"v{base + 10}", f"v{base + 11}"
    assert base + 12 <= 32
    for q in range(4):
        for j in range(0, n, 2):
            for (jj, x, y, kx, ky) in ((j, x0, y0, k0, k1), (j + 1, x1, y1, k2, k3)):
                off = 128 * jj + 16 * q
                g.emit(f"ldr q{x[1:]}, [{dreg}, #{off}]")
                g.emit(f"ldr q{y[1:]}, [{dreg}, #{off + 64}]")
                g.emit(f"ldr q{kx[1:]}, [{kreg}, #{off}]")
                g.emit(f"ldr q{ky[1:]}, [{kreg}, #{off + 64}]")
                g.emit(f"eor {kx}.16b, {x}.16b, {kx}.16b")
                g.emit(f"eor {ky}.16b, {y}.16b, {ky}.16b")
                g.emit(f"pmull {p0}.1q, {kx}.1d, {ky}.1d")
                g.emit(f"pmull2 {p1}.1q, {kx}.2d, {ky}.2d")
                g.emit(f"eor3 {acc[jj]}.16b, {acc[jj]}.16b, {p0}.16b, {p1}.16b")
            if j == 0:
                g.emit(f"eor {px}.16b, {x0}.16b, {x1}.16b")
                g.emit(f"eor {py}.16b, {y0}.16b, {y1}.16b")
            else:
                g.emit(f"eor3 {px}.16b, {px}.16b, {x0}.16b, {x1}.16b")
                g.emit(f"eor3 {py}.16b, {py}.16b, {y0}.16b, {y1}.16b")
        off = 128 * n + 16 * q
        g.emit(f"ldr q{k0[1:]}, [{kreg}, #{off}]")
        g.emit(f"ldr q{k1[1:]}, [{kreg}, #{off + 64}]")
        g.emit(f"eor {k0}.16b, {px}.16b, {k0}.16b")
        g.emit(f"eor {k1}.16b, {py}.16b, {k1}.16b")
        g.emit(f"pmull {p0}.1q, {k0}.1d, {k1}.1d")
        g.emit(f"pmull2 {p1}.1q, {k0}.2d, {k1}.2d")
        g.emit(f"eor3 {acc[n]}.16b, {acc[n]}.16b, {p0}.16b, {p1}.16b")


def chain_uv_stripe(g, n, dreg="{d}", kreg="{k}", nt=False):
    """Chain code, n = 8*U blocks; position j = 8u + v accumulates the block
    product sum into B[v] (v0..v7) and C[u] (v8..v15). Endpoint -> memory E."""
    assert n % 8 == 0 and n // 8 <= 8
    B = [f"v{i}" for i in range(8)]
    C = [f"v{8 + i}" for i in range(8)]
    pool = [f"v{i}" for i in range(16, 26)]
    kx, ky, p0, p1, S = "v26", "v27", "v28", "v29", "v30"
    spare = "v31"
    ld = "ldr"
    prev = {}
    prods = []
    for j in range(n + 1):
        u, v = divmod(j, 8)
        for q in range(4):
            koff = 128 * j + 16 * q
            g.emit(f"ldr q{kx[1:]}, [{kreg}, #{koff}]")
            g.emit(f"ldr q{ky[1:]}, [{kreg}, #{koff + 64}]")
            if j < n:
                doff = 128 * j + 16 * q
                x = pool.pop(0)
                y = pool.pop(0)
                g.emit(f"{ld} q{x[1:]}, [{dreg}, #{doff}]")
                g.emit(f"{ld} q{y[1:]}, [{dreg}, #{doff + 64}]")
            if j == 0:
                g.emit(f"eor {kx}.16b, {x}.16b, {kx}.16b")
                g.emit(f"eor {ky}.16b, {y}.16b, {ky}.16b")
            elif j < n:
                px, py = prev[q]
                g.emit(f"eor3 {kx}.16b, {px}.16b, {x}.16b, {kx}.16b")
                g.emit(f"eor3 {ky}.16b, {py}.16b, {y}.16b, {ky}.16b")
                pool += [px, py]
            else:
                px, py = prev[q]
                g.emit(f"eor {kx}.16b, {px}.16b, {kx}.16b")
                g.emit(f"eor {ky}.16b, {py}.16b, {ky}.16b")
                pool += [px, py]
            if j == n:
                # endpoint: accumulate into spare, stored to E later
                g.emit(f"pmull {p0}.1q, {kx}.1d, {ky}.1d")
                g.emit(f"pmull2 {p1}.1q, {kx}.2d, {ky}.2d")
                if q == 0:
                    g.emit(f"eor {spare}.16b, {p0}.16b, {p1}.16b")
                else:
                    g.emit(f"eor3 {spare}.16b, {spare}.16b, {p0}.16b, {p1}.16b")
            else:
                g.emit(f"pmull {p0}.1q, {kx}.1d, {ky}.1d")
                g.emit(f"pmull2 {p1}.1q, {kx}.2d, {ky}.2d")
                if q == 0:
                    g.emit(f"eor {S}.16b, {p0}.16b, {p1}.16b")
                elif q < 3:
                    g.emit(f"eor3 {S}.16b, {S}.16b, {p0}.16b, {p1}.16b")
                else:
                    g.emit(f"eor3 {S}.16b, {S}.16b, {p0}.16b, {p1}.16b")
                    g.emit(f"eor {B[v]}.16b, {B[v]}.16b, {S}.16b")
                    g.emit(f"eor {C[u]}.16b, {C[u]}.16b, {S}.16b")
                prev[q] = (x, y)
    g.emit(f"ldr q{S[1:]}, [{{e}}]")
    g.emit(f"eor {S}.16b, {S}.16b, {spare}.16b")
    g.emit(f"str q{S[1:]}, [{{e}}]")


def kernel_uv(name, n):
    g = Gen()
    chain_uv_stripe(g, n)
    body = "\n".join(f'                "{l}",' for l in g.lines)
    load_acc = "\n".join(f'                "ldr q{i}, [{{a}}, #{16 * i}]",' for i in range(16))
    store_acc = "\n".join(f'                "str q{i}, [{{a}}, #{16 * i}]",' for i in range(16))
    clob = ", ".join(f'out("v{i}") _' for i in range(32))
    return f'''
/// {name}: `chunks` chunks, each one stripe of {n} blocks, u/v accumulators.
#[inline(never)]
pub unsafe fn {name}(acc: &mut [u128; 32], data: *const u8, key: *const u8, chunks: usize) {{
    unsafe {{
        let e = acc.as_mut_ptr().add(16);
        core::arch::asm!(
{load_acc}
            "3:",
{body}
                "add {{d}}, {{d}}, #{128 * n}",
                "subs {{chunks}}, {{chunks}}, #1",
                "b.ne 3b",
{store_acc}
            a = in(reg) acc.as_mut_ptr(),
            e = in(reg) e,
            d = inout(reg) data => _,
            k = in(reg) key,
            chunks = inout(reg) chunks => _,
            {clob},
            options(nostack),
        );
    }}
}}
'''


def chain_v2_body(g, ldd="ldp", keys_first=False):
    """8 blocks (one u iteration) of the chain code with grouped data loads.

    Registers: B[v] v0..v7, C_cur v8, row sets P v9..v16 / N v17..v24
    (x0..x3, y0..y3), keys v25..v28, products v29,v30, block sum v31.
    P holds the previous block's rows; roles swap every block (8 is even).
    """
    B = [f"v{i}" for i in range(8)]
    Cc = "v8"
    sets = [[f"v{9 + i}" for i in range(8)], [f"v{17 + i}" for i in range(8)]]
    k = ["v25", "v26", "v27", "v28"]
    p0, p1, S = "v29", "v30", "v31"
    for b in range(8):
        P, N = sets[b % 2], sets[(b + 1) % 2]
        doff = 128 * b
        kl = []
        for q in range(0, 4, 2):
            kl.append(f"ldp q{k[0][1:]}, q{k[1][1:]}, [{{k}}, #{doff + 16 * q}]")
        if keys_first:
            pass
        # data: x0,x1 | x2,x3 | y0,y1 | y2,y3
        dl = []
        for (i, off) in ((0, 0), (2, 32), (4, 64), (6, 96)):
            if ldd == "ldp":
                dl.append(f"ldp q{N[i][1:]}, q{N[i + 1][1:]}, [{{d}}, #{doff + off}]")
            else:
                dl.append(f"ldnp q{N[i][1:]}, q{N[i + 1][1:]}, [{{d}}, #{doff + off}]")
        for l in dl:
            g.emit(l)
        for qp in (0, 2):
            # keys for columns qp, qp+1: kx pair and ky pair
            g.emit(f"ldp q{k[0][1:]}, q{k[1][1:]}, [{{k}}, #{doff + 16 * qp}]")
            g.emit(f"ldp q{k[2][1:]}, q{k[3][1:]}, [{{k}}, #{doff + 64 + 16 * qp}]")
            for (q, kx, ky) in ((qp, k[0], k[2]), (qp + 1, k[1], k[3])):
                g.emit(f"eor3 {kx}.16b, {P[q]}.16b, {N[q]}.16b, {kx}.16b")
                g.emit(f"eor3 {ky}.16b, {P[4 + q]}.16b, {N[4 + q]}.16b, {ky}.16b")
                g.emit(f"pmull {p0}.1q, {kx}.1d, {ky}.1d")
                g.emit(f"pmull2 {p1}.1q, {kx}.2d, {ky}.2d")
                if q == 0:
                    g.emit(f"eor {S}.16b, {p0}.16b, {p1}.16b")
                else:
                    g.emit(f"eor3 {S}.16b, {S}.16b, {p0}.16b, {p1}.16b")
        g.emit(f"eor {B[b]}.16b, {B[b]}.16b, {S}.16b")
        g.emit(f"eor {Cc}.16b, {Cc}.16b, {S}.16b")


def kernel_v2(name, ldd="ldp", mode="normal"):
    g = Gen()
    chain_v2_body(g, ldd)
    if mode == "loadsonly":
        g.lines = [l for l in g.lines if l.startswith("ld")]
    elif mode == "keysfirst":
        # move each block's key loads ahead of its data loads
        out, blk = [], []
        for l in g.lines:
            blk.append(l)
            if l.startswith("eor v8.16b"):
                data = [x for x in blk if x.startswith("ldp") and "{d}" in x]
                keys = [x for x in blk if x.startswith("ldp") and "{k}" in x]
                rest = [x for x in blk if not x.startswith("ldp")]
                # keys need 8 registers; only 4 exist, so hoist the first pair only
                out += keys[:2] + data + keys[2:] + rest if False else keys[:1] + data + [x for x in blk if x not in keys[:1] and x not in data]
                blk = []
        g.lines = out
    body = "\n".join(f'                "{l}",' for l in g.lines)
    load_acc = "\n".join(f'                "ldr q{i}, [{{a}}, #{16 * i}]",' for i in range(8))
    store_acc = "\n".join(f'                "str q{i}, [{{a}}, #{16 * i}]",' for i in range(8))
    clob = ", ".join(f'out("v{i}") _' for i in range(32))
    # endpoint: previous rows are in set index (8 % 2) = 0 -> v9..v16
    ep = []
    for qp in (0, 2):
        ep.append(f"ldp q25, q26, [{{k}}, #{16 * qp}]")
        ep.append(f"ldp q27, q28, [{{k}}, #{64 + 16 * qp}]")
        for (q, kx, ky) in ((qp, "v25", "v27"), (qp + 1, "v26", "v28")):
            ep.append(f"eor {kx}.16b, v{9 + q}.16b, {kx}.16b")
            ep.append(f"eor {ky}.16b, v{13 + q}.16b, {ky}.16b")
            ep.append(f"pmull v29.1q, {kx}.1d, {ky}.1d")
            ep.append(f"pmull2 v30.1q, {kx}.2d, {ky}.2d")
            ep.append(f"eor3 v31.16b, v31.16b, v29.16b, v30.16b")
    epb = "\n".join(f'                "{l}",' for l in ep)
    return f'''
/// {name}: `chunks` chunks, each a 64-block stripe; u-loop, grouped loads.
#[inline(never)]
pub unsafe fn {name}(acc: &mut [u128; 32], data: *const u8, key: *const u8, chunks: usize) {{
    unsafe {{
        core::arch::asm!(
{load_acc}
            "ldr q8, [{{a}}, #128]",
            "3:",
                "mov {{k}}, {{k0}}",
                "mov {{u}}, #8",
                "movi v9.16b, #0", "movi v10.16b, #0", "movi v11.16b, #0", "movi v12.16b, #0",
                "movi v13.16b, #0", "movi v14.16b, #0", "movi v15.16b, #0", "movi v16.16b, #0",
            "2:",
{body}
                "str q8, [{{c}}]",
                "add {{d}}, {{d}}, #1024",
                "add {{k}}, {{k}}, #1024",
                "subs {{u}}, {{u}}, #1",
                "b.ne 2b",
                "movi v31.16b, #0",
{epb}
                "ldr q30, [{{c}}, #16]",
                "eor v30.16b, v30.16b, v31.16b",
                "str q30, [{{c}}, #16]",
                "subs {{chunks}}, {{chunks}}, #1",
                "b.ne 3b",
{store_acc}
            a = in(reg) acc.as_mut_ptr(),
            c = in(reg) acc.as_mut_ptr().add(8),
            d = inout(reg) data => _,
            k0 = in(reg) key,
            k = out(reg) _,
            u = out(reg) _,
            chunks = inout(reg) chunks => _,
            {clob},
            options(nostack),
        );
    }}
}}
'''


def kernel_multi(name, stripe_fn, n, stripes, data_stride, key_stride, nacc):
    """Loop over `chunks` chunks inside the asm; key pointer rewinds per chunk."""
    g = Gen()
    stripe_fn(g, n)
    body = "\n".join(f'                "{l}",' for l in g.lines)
    clob = ", ".join(f'out("v{i}") _' for i in range(nacc, 32))
    load_acc = "\n".join(
        f'                "ldr q{i}, [{{a}}, #{16 * i}]",' for i in range(nacc))
    store_acc = "\n".join(
        f'                "str q{i}, [{{a}}, #{16 * i}]",' for i in range(nacc))
    acc_clob = ", ".join(f'out("v{i}") _' for i in range(nacc))
    return f'''
/// {name}: `chunks` chunks of {stripes} stripe(s) of {n} blocks.
#[inline(never)]
pub unsafe fn {name}(acc: &mut [u128; 32], data: *const u8, key: *const u8, chunks: usize) {{
    unsafe {{
        core::arch::asm!(
{load_acc}
            "3:",
                "mov {{k}}, {{k0}}",
                "mov {{cnt}}, #{stripes}",
            "2:",
{body}
                "add {{d}}, {{d}}, #{data_stride}",
                "add {{k}}, {{k}}, #{key_stride}",
                "subs {{cnt}}, {{cnt}}, #1",
                "b.ne 2b",
                "subs {{chunks}}, {{chunks}}, #1",
                "b.ne 3b",
{store_acc}
            a = in(reg) acc.as_mut_ptr(),
            d = inout(reg) data => _,
            k0 = in(reg) key,
            k = out(reg) _,
            cnt = out(reg) _,
            chunks = inout(reg) chunks => _,
            {acc_clob}, {clob},
            options(nostack),
        );
    }}
}}
'''


def kernel(name, stripe_fn, n, stripes, data_stride, key_stride, nacc):
    g = Gen()
    stripe_fn(g, n)
    body = "\n".join(f'                "{l}",' for l in g.lines)
    clob = ", ".join(f'out("v{i}") _' for i in range(nacc, 32))
    load_acc = "\n".join(
        f'                "ldr q{i}, [{{a}}, #{16 * i}]",' for i in range(nacc))
    store_acc = "\n".join(
        f'                "str q{i}, [{{a}}, #{16 * i}]",' for i in range(nacc))
    acc_clob = ", ".join(f'out("v{i}") _' for i in range(nacc))
    return f'''
/// {name}: {stripes} stripe(s) of {n} blocks per chunk.
#[inline(never)]
pub unsafe fn {name}(acc: &mut [u128; 32], data: *const u8, key: *const u8) {{
    unsafe {{
        core::arch::asm!(
{load_acc}
            "2:",
{body}
                "add {{d}}, {{d}}, #{data_stride}",
                "add {{k}}, {{k}}, #{key_stride}",
                "subs {{cnt}}, {{cnt}}, #1",
                "b.ne 2b",
{store_acc}
            a = in(reg) acc.as_mut_ptr(),
            d = inout(reg) data => _,
            k = inout(reg) key => _,
            cnt = inout(reg) {stripes}u64 => _,
            {acc_clob}, {clob},
            options(nostack),
        );
    }}
}}
'''


def main():
    out = ["//! Generated by gen_asm.py; do not edit by hand.\n#![allow(clippy::all)]\n"]
    for n, stripes in ((8, 4), (12, 3), (16, 2)):
        out.append(kernel(f"asm_chain{n}", chain_stripe, n, stripes, 128 * n, 128 * (n + 1), n + 1))
    for n, stripes in ((4, 8), (8, 4), (16, 2)):
        out.append(kernel(f"asm_parity{n}", parity_stripe, n, stripes, 128 * n, 128 * (n + 1), n + 1))
    out.append(kernel_multi("asm_chain16_multi", chain_stripe, 16, 2, 128 * 16, 128 * 17, 17))
    out.append(kernel_uv("asm_chain64uv", 64))
    out.append(kernel_uv("asm_chain32uv", 32))
    out.append(kernel_v2("asm_chain64v2"))
    out.append(kernel_v2("asm_chain64v2nt", "ldnp"))
    out.append(kernel_v2("asm_chain64v2lo", mode="loadsonly"))
    out.append(kernel_v2("asm_chain64v2kf", mode="keysfirst"))
    out.append(kernel_multi("asm_parity8_multi", parity_stripe, 8, 4, 128 * 8, 128 * 9, 9))
    with open(os.path.join(HERE, "src", "asm_kernels.rs"), "w") as f:
        f.write("".join(out))


if __name__ == "__main__":
    main()

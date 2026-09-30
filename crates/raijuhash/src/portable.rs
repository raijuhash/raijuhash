//! Portable backend: identical output to the SIMD paths, using ordinary
//! integer arithmetic only.

use crate::Key;
use crate::params::{BLOCK, CHUNK_BLOCKS};
use crate::reference::reduce256;
use crate::state::{Core, padded_block};

/// Carryless 32x32 product using integer multiplication on operands split into
/// four interleaved bit classes. Each class keeps three zero bits between
/// set bits, which absorb the carries of at most eight summed terms, so no
/// carry reaches a bit of the same class. No secret-dependent branches.
#[inline(always)]
fn clmul32(x: u32, y: u32) -> u64 {
    const M: [u64; 4] = [0x1111_1111, 0x2222_2222, 0x4444_4444, 0x8888_8888];
    const W: [u64; 4] = [
        0x1111_1111_1111_1111,
        0x2222_2222_2222_2222,
        0x4444_4444_4444_4444,
        0x8888_8888_8888_8888,
    ];
    let (x, y) = (x as u64, y as u64);
    let xs = M.map(|m| x & m);
    let ys = M.map(|m| y & m);
    let mut z = 0;
    for k in 0..4 {
        let mut t = 0;
        for i in 0..4 {
            t ^= xs[i].wrapping_mul(ys[(k + 4 - i) % 4]);
        }
        z |= t & W[k];
    }
    z
}

/// Carryless 64x64 product by Karatsuba over 32-bit halves.
#[inline(always)]
pub fn clmul64(a: u64, b: u64) -> u128 {
    let (a0, a1) = (a as u32, (a >> 32) as u32);
    let (b0, b1) = (b as u32, (b >> 32) as u32);
    let lo = clmul32(a0, b0) as u128;
    let hi = clmul32(a1, b1) as u128;
    let mid = clmul32(a0 ^ a1, b0 ^ b1) as u128 ^ lo ^ hi;
    lo ^ (mid << 32) ^ (hi << 64)
}

pub fn gf_mul(a: u128, b: u128) -> u128 {
    let (a0, a1) = (a as u64, (a >> 64) as u64);
    let (b0, b1) = (b as u64, (b >> 64) as u64);
    let lo = clmul64(a0, b0);
    let hi = clmul64(a1, b1);
    let mid = clmul64(a0 ^ a1, b0 ^ b1) ^ lo ^ hi;
    reduce256(lo ^ (mid << 64), hi ^ (mid >> 64))
}

/// `sum_k x^k * z_k` for k < 8, reduced.
fn shifted_sum(z: &[u128]) -> u128 {
    let (mut low, mut high) = (0u128, 0u128);
    for (k, &v) in z.iter().enumerate() {
        low ^= v << k;
        if k > 0 {
            high ^= v >> (128 - k);
        }
    }
    reduce256(low, high)
}

fn fold(b: &[u128; 8], c: &[u128; 8], e: u128) -> (u128, u128) {
    let sel = |a: &[u128; 8], bit: usize| {
        (0..8).filter(|i| i >> bit & 1 == 1).fold(0, |acc, i| acc ^ a[i])
    };
    let h0 = b.iter().fold(0, |acc, v| acc ^ v);
    let h1 = shifted_sum(&[e ^ sel(b, 0), sel(b, 1), sel(b, 2), sel(c, 0), sel(c, 1), sel(c, 2)]);
    (h0, h1)
}

fn word(p: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(p[8 * i..8 * i + 8].try_into().unwrap())
}

fn row_word(key: &Key, j: usize, i: usize) -> u64 {
    // SAFETY: row `j` is final (see `Key::ensure_rows`); read without a
    // reference to the table.
    u64::from_le(unsafe { core::ptr::read_unaligned(key.rows().add(BLOCK * j + 8 * i) as *const u64) })
}

/// Product sum of the encoded symbols at position `j` given the rows before
/// (`prev`) and at (`cur`) that position; either may be all zero.
fn position_sum(key: &Key, j: usize, prev: &[u8], cur: &[u8]) -> u128 {
    let mut s = 0;
    for l in 0..8 {
        let tx = word(prev, l) ^ word(cur, l) ^ row_word(key, j, l);
        let ty = word(prev, 8 + l) ^ word(cur, 8 + l) ^ row_word(key, j, 8 + l);
        s ^= clmul64(tx, ty);
    }
    s
}

/// Chunk sums and previous block, unpacked from the state words.
struct Sums {
    b: [u128; 8],
    c: [u128; 8],
    prev: [u8; BLOCK],
}

impl Sums {
    fn load(core: &Core) -> Sums {
        let w = core.words().copied().unwrap_or([0; 24]);
        let mut prev = [0u8; BLOCK];
        for i in 0..8 {
            prev[16 * i..16 * i + 16].copy_from_slice(&w[16 + i].to_le_bytes());
        }
        Sums { b: w[..8].try_into().unwrap(), c: w[8..16].try_into().unwrap(), prev }
    }

    fn store(&self, core: &mut Core) {
        let w = core.words_mut();
        w[..8].copy_from_slice(&self.b);
        w[8..16].copy_from_slice(&self.c);
        for i in 0..8 {
            w[16 + i] = u128::from_le_bytes(self.prev[16 * i..16 * i + 16].try_into().unwrap());
        }
    }

    fn absorb(&mut self, key: &Key, j: usize, blk: &[u8]) {
        let s = position_sum(key, j, &self.prev, blk);
        self.b[j % 8] ^= s;
        self.c[j / 8] ^= s;
        self.prev.copy_from_slice(blk);
    }

    /// End position `j`, then `(outer + h0) R + h1 R2`.
    fn close(&mut self, key: &Key, j: usize, outer: u128) -> u128 {
        let s = position_sum(key, j, &self.prev, &[0; BLOCK]);
        let mut e = 0;
        if j == CHUNK_BLOCKS {
            e = s;
        } else {
            self.b[j % 8] ^= s;
            self.c[j / 8] ^= s;
        }
        let (h0, h1) = fold(&self.b, &self.c, e);
        gf_mul(outer ^ h0, key.r.k) ^ gf_mul(h1, key.r2.k)
    }
}

pub fn groups(core: &mut Core, key: &Key, data: &[u8]) {
    let mut sums = Sums::load(core);
    for (i, blk) in data.chunks_exact(BLOCK).enumerate() {
        sums.absorb(key, core.pos + i, blk);
    }
    let pos = core.pos + data.len() / BLOCK;
    if pos == CHUNK_BLOCKS {
        core.outer = sums.close(key, pos, core.outer);
        core.pos = 0;
        core.closed = true;
    } else {
        sums.store(core);
        core.pos = pos;
    }
}

pub fn finish(core: &Core, key: &Key, pending: &[u8], len: u64) -> u128 {
    let mut sums = Sums::load(core);
    let mut j = core.pos;
    for blk in pending.chunks(BLOCK) {
        let padded = padded_block_or_copy(blk);
        sums.absorb(key, j, &padded);
        j += 1;
    }
    let outer = if j == 0 { core.outer } else { sums.close(key, j, core.outer) };
    outer ^ gf_mul(len as u128, key.t.k) ^ key.s
}

fn padded_block_or_copy(blk: &[u8]) -> [u8; BLOCK] {
    if blk.len() == BLOCK { blk.try_into().unwrap() } else { padded_block(blk) }
}

pub fn short(key: &Key, x0: u128, x1: Option<u128>) -> u128 {
    gf_mul(x0, key.a.k) ^ x1.map_or(0, |x1| gf_mul(x1, key.b.k)) ^ key.s
}

//! CANDIDATES.md X18: carryless 64x64 multiplication without CLMUL. Current
//! (`portable::clmul64`): Karatsuba over three 32x32 products, each with
//! sixteen 64-bit multiplies of four interleaved bit classes. Candidate: five
//! bit classes of each 64-bit operand (every fifth bit), 25 multiplies
//! 64x64->128; at most 13 terms meet at a bit, below the 2^4 a 5-bit gap holds.
use std::hint::black_box;
use std::time::Instant;

fn clmul32(x: u32, y: u32) -> u64 {
    const M: [u64; 4] = [0x1111_1111, 0x2222_2222, 0x4444_4444, 0x8888_8888];
    const W: [u64; 4] = [0x1111_1111_1111_1111, 0x2222_2222_2222_2222, 0x4444_4444_4444_4444, 0x8888_8888_8888_8888];
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

fn clmul64_current(a: u64, b: u64) -> u128 {
    let (a0, a1) = (a as u32, (a >> 32) as u32);
    let (b0, b1) = (b as u32, (b >> 32) as u32);
    let lo = clmul32(a0, b0) as u128;
    let hi = clmul32(a1, b1) as u128;
    let mid = clmul32(a0 ^ a1, b0 ^ b1) as u128 ^ lo ^ hi;
    lo ^ (mid << 32) ^ (hi << 64)
}

/// Every fifth bit of a 64-bit word, starting at bit `c`.
const fn class_mask(c: u32) -> u64 {
    let mut m = 0u64;
    let mut i = c;
    while i < 64 {
        m |= 1 << i;
        i += 5;
    }
    m
}

fn class_mask128(c: u32) -> u128 {
    let mut m = 0u128;
    let mut i = c;
    while i < 128 {
        m |= 1 << i;
        i += 5;
    }
    m
}

#[inline(always)]
fn clmul64_five(a: u64, b: u64) -> u128 {
    const M: [u64; 5] = [class_mask(0), class_mask(1), class_mask(2), class_mask(3), class_mask(4)];
    let w: [u128; 5] = core::array::from_fn(|c| class_mask128(c as u32));
    let xs = M.map(|m| (a & m) as u128);
    let ys = M.map(|m| (b & m) as u128);
    let mut z = 0u128;
    for k in 0..5 {
        let mut t = 0u128;
        for i in 0..5 {
            t ^= xs[i].wrapping_mul(ys[(k + 5 - i) % 5]);
        }
        z |= t & w[k];
    }
    z
}

fn clmul64_bits(a: u64, b: u64) -> u128 {
    (0..64).filter(|i| b >> i & 1 == 1).fold(0, |r, i| r ^ (a as u128) << i)
}

fn main() {
    let mut s = 0x243f_6a88_85a3_08d3u64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for _ in 0..200_000 {
        let (a, b) = (next(), next());
        let r = clmul64_bits(a, b);
        assert_eq!(clmul64_current(a, b), r);
        assert_eq!(clmul64_five(a, b), r);
    }
    for (a, b) in [(u64::MAX, u64::MAX), (u64::MAX, 1), (1 << 63, 1 << 63)] {
        assert_eq!(clmul64_five(a, b), clmul64_bits(a, b));
    }
    println!("both match the bitwise product (200k random pairs and extremes)");
    let v: Vec<(u64, u64)> = (0..4096).map(|_| (next(), next())).collect();
    let time = |f: &dyn Fn(u64, u64) -> u128| {
        let mut best = f64::INFINITY;
        for _ in 0..15 {
            let t = Instant::now();
            let mut acc = 0u128;
            for _ in 0..100 {
                for &(a, b) in &v {
                    acc ^= f(black_box(a), black_box(b));
                }
            }
            black_box(acc);
            best = best.min(t.elapsed().as_secs_f64() * 1e9 / (100.0 * v.len() as f64));
        }
        best
    };
    let c = time(&clmul64_current);
    let f = time(&clmul64_five);
    println!("ns per 64x64 carryless product: current {c:.2}, five classes {f:.2} ({:.2}x)", c / f);
}

//! RaijuHash against other fast universal hashes, one message per call.
//! Keys are prepared outside the timer for all candidates except HalftimeHash16,
//! whose one-shot `digest_master_key` API performs master-key setup per call.
//! Other candidates are POLYVAL and GHASH
//! (RustCrypto, with a length block appended), Poly1305 (RustCrypto,
//! `compute_unpadded`) and the 128-bit UMASH fingerprint (the C library through
//! `umash-sys`).
//!
//! Every hash is called through its own non-inlined function, so the
//! harness code around each call is the same, and the hashes are timed in
//! turn at every size (best of several batches each).
//!
//! Usage: axu [--quick]. Prints fixed sizes (ns per message and GB/s),
//! random lengths (ns per message) and key setup (ns).
use std::hint::black_box;
use std::time::Instant;

use ghash::GHash;
use halftime::{HalftimeHash16, Key32 as HalftimeKey32};
use poly1305::Poly1305;
use polyval::Polyval;
use polyval::universal_hash::{KeyInit, UniversalHash};
use raijuhash::Key;
use umash_sys::{umash_fprint, umash_params, umash_params_derive};

const NAMES: [&str; 6] = [
    "RaijuHash",
    "POLYVAL",
    "GHASH",
    "Poly1305",
    "UMASH-128",
    "HalftimeHash16",
];

struct Hashes {
    raiju: Key,
    polyval: Polyval,
    ghash: GHash,
    poly1305: Poly1305,
    umash: Box<umash_params>,
    halftime_key: HalftimeKey32,
}

impl Hashes {
    fn new(seed: u64) -> Hashes {
        let k16 = (seed as u128 * 0x9e37_79b9_7f4a_7c15).to_le_bytes();
        let mut k32 = [0u8; 32];
        k32[..16].copy_from_slice(&k16);
        k32[16..].copy_from_slice(&k16.map(|b| b ^ 0x5a));
        Hashes {
            raiju: Key::from_seed(seed as u128),
            polyval: Polyval::new(&k16.into()),
            ghash: GHash::new(&k16.into()),
            poly1305: Poly1305::new(&k32.into()),
            umash: umash_params_from(&k32),
            halftime_key: HalftimeKey32::from([0x3c; 32]),
        }
    }
}

fn umash_params_from(key: &[u8; 32]) -> Box<umash_params> {
    // SAFETY: `umash_params_derive` fills the whole struct from 32 key bytes.
    unsafe {
        let mut p: Box<umash_params> = Box::new(std::mem::zeroed());
        umash_params_derive(&mut *p, 0, key.as_ptr().cast());
        p
    }
}

/// The 16-byte length block appended to POLYVAL and GHASH, so that they
/// hash variable-length messages (as GCM and GCM-SIV do).
fn len_block(len: usize) -> [u8; 16] {
    ((len as u128) << 3).to_le_bytes()
}

#[inline(never)]
fn raiju(h: &Hashes, m: &[u8]) -> u128 {
    h.raiju.hash(m)
}

#[inline(never)]
fn polyval(h: &Hashes, m: &[u8]) -> [u8; 16] {
    let mut s = h.polyval.clone();
    s.update_padded(m);
    s.update_padded(&len_block(m.len()));
    s.finalize().into()
}

#[inline(never)]
fn ghash(h: &Hashes, m: &[u8]) -> [u8; 16] {
    let mut s = h.ghash.clone();
    s.update_padded(m);
    s.update_padded(&len_block(m.len()));
    s.finalize().into()
}

#[inline(never)]
fn poly1305(h: &Hashes, m: &[u8]) -> [u8; 16] {
    h.poly1305.clone().compute_unpadded(m).into()
}

#[inline(never)]
fn umash(h: &Hashes, m: &[u8]) -> [u64; 2] {
    // SAFETY: `m` is valid for `m.len()` bytes; the params are initialized.
    unsafe { umash_fprint(&*h.umash, 0, m.as_ptr().cast(), m.len() as _).hash }
}

#[inline(never)]
fn halftime(h: &Hashes, m: &[u8]) -> halftime::universal_hash::Block<HalftimeHash16> {
    HalftimeHash16::digest_master_key(&h.halftime_key, m)
}

/// Hash `m` with hash number `i` (in `NAMES` order).
fn call(h: &Hashes, i: usize, m: &[u8]) {
    match i {
        0 => {
            black_box(raiju(h, black_box(m)));
        }
        1 => {
            black_box(polyval(h, black_box(m)));
        }
        2 => {
            black_box(ghash(h, black_box(m)));
        }
        3 => {
            black_box(poly1305(h, black_box(m)));
        }
        4 => {
            black_box(umash(h, black_box(m)));
        }
        _ => {
            black_box(halftime(h, black_box(m)));
        }
    }
}

/// Seconds per call of `f`: best of `rounds` batches of about `target`
/// seconds each, after a warm-up of one batch.
fn time_s(f: &mut dyn FnMut(), target: f64, rounds: usize) -> f64 {
    let t = Instant::now();
    let mut n = 0u64;
    while t.elapsed().as_secs_f64() < target {
        f();
        n += 1;
    }
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() / n as f64);
    }
    best
}

/// A zeroed-then-patterned buffer whose data starts 64-byte aligned.
fn buffer(len: usize) -> (Vec<u8>, usize) {
    let mut buf: Vec<u8> = (0..len + 64).map(|i| (i * 131 + 7) as u8).collect();
    let off = (64 - buf.as_ptr() as usize % 64) % 64;
    buf.truncate(off + len);
    (buf, off)
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let (target, rounds, passes) = if quick { (0.005, 3, 1) } else { (0.02, 5, 3) };
    let h = Hashes::new(0x0123_4567_89ab_cdef);
    println!(
        "arch {}; hashes: {}",
        std::env::consts::ARCH,
        NAMES.join(", ")
    );
    println!("RaijuHash backend: {:?}", h.raiju.backend());
    // Half a second of work first, so that the core is at full clock (on
    // macOS the thread cannot be pinned) before the first measurement.
    let (warm, off) = buffer(4096);
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < 0.5 {
        for i in 0..NAMES.len() {
            call(&h, i, &warm[off..]);
        }
    }

    println!("\n## Fixed sizes: ns per message (GB/s in parentheses from 1 KiB)");
    println!("| bytes | {} |", NAMES.join(" | "));
    let sizes = [
        8usize,
        16,
        32,
        64,
        128,
        256,
        512,
        1024,
        4096,
        16384,
        65536,
        262_144,
        1 << 20,
        16 << 20,
        64 << 20,
    ];
    for &len in &sizes {
        let (buf, off) = buffer(len);
        let m = &buf[off..off + len];
        let mut best = [f64::INFINITY; NAMES.len()];
        for _ in 0..passes {
            for (i, b) in best.iter_mut().enumerate() {
                *b = b.min(time_s(&mut || call(&h, i, m), target, rounds));
            }
        }
        let cells: Vec<String> = best
            .iter()
            .map(|&t| {
                if len >= 1024 {
                    format!("{:.1} ({:.1})", t * 1e9, len as f64 / t / 1e9)
                } else {
                    format!("{:.2}", t * 1e9)
                }
            })
            .collect();
        println!("| {len} | {} |", cells.join(" | "));
    }

    println!("\n## Random lengths and offsets (2^16 messages): ns per message");
    println!("| lengths | {} |", NAMES.join(" | "));
    let (buf, off) = buffer(1 << 17);
    let data = &buf[off..];
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as usize
    };
    for (lo, hi) in [(1usize, 64usize), (1, 256), (1, 1024)] {
        let cases: Vec<(usize, usize)> = (0..1 << 16)
            .map(|_| (next() % (1 << 16), lo + next() % (hi - lo + 1)))
            .collect();
        let mut best = [f64::INFINITY; NAMES.len()];
        for _ in 0..passes {
            for (i, b) in best.iter_mut().enumerate() {
                let t = time_s(
                    &mut || {
                        for &(o, l) in &cases {
                            call(&h, i, &data[o..o + l])
                        }
                    },
                    target,
                    rounds,
                );
                *b = b.min(t / cases.len() as f64);
            }
        }
        let cells: Vec<String> = best.iter().map(|&t| format!("{:.2}", t * 1e9)).collect();
        println!("| {lo}-{hi} | {} |", cells.join(" | "));
    }

    println!("\n## Key setup: ns per key");
    let k16 = [7u8; 16];
    let k32 = [7u8; 32];
    let setup: [(&str, &mut dyn FnMut()); NAMES.len()] = [
        ("RaijuHash (Key::from_seed)", &mut || {
            black_box(&Key::from_seed(black_box(7)));
        }),
        ("POLYVAL (new)", &mut || {
            black_box(&Polyval::new(black_box(&k16.into())));
        }),
        ("GHASH (new)", &mut || {
            black_box(&GHash::new(black_box(&k16.into())));
        }),
        ("Poly1305 (new)", &mut || {
            black_box(&Poly1305::new(black_box(&k32.into())));
        }),
        ("UMASH (umash_params_derive)", &mut || {
            // SAFETY: as in `umash_params_from`, into a stack value.
            let mut p: umash_params = unsafe { std::mem::zeroed() };
            unsafe { umash_params_derive(&mut p, 0, black_box(&k32).as_ptr().cast()) };
            black_box(&p);
        }),
        ("HalftimeHash16 (from_master_key)", &mut || {
            black_box(HalftimeHash16::from_master_key(black_box(&h.halftime_key)));
        }),
    ];
    for (name, f) in setup {
        println!("| {name} | {:.1} |", time_s(f, target, rounds) * 1e9);
    }
}

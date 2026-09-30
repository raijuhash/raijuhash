//! Keyed hash throughput reference points for hash-table style use.
//!
//! This compares RaijuHash's avalanche output, truncated to the conventional
//! 64-bit table-hash width, with SipHash-2-4 and HighwayHash64. The RaijuHash
//! API explicitly forbids exposing its hash modes to an adversary who chooses
//! messages, so these are timing comparisons, not a security-equivalence test
//! or a recommendation to use RaijuHash as a HashMap BuildHasher.
//!
//! Usage: `keyed [--quick]`.

use std::hash::Hasher;
use std::hint::black_box;
use std::time::Instant;

use highway::{HighwayHash, HighwayHasher, Key as HighwayKey};
use raijuhash::Key;
use siphasher::sip::SipHasher24;

const NAMES: [&str; 3] = ["RaijuHash-avalanche-64", "SipHash-2-4", "HighwayHash64"];

struct Hashes {
    raiju: Key,
    sip: (u64, u64),
    highway: HighwayKey,
}

impl Hashes {
    fn new() -> Self {
        Self {
            raiju: Key::from_seed(0x0123_4567_89ab_cdef),
            sip: (0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210),
            highway: HighwayKey([
                0x0123_4567_89ab_cdef,
                0xfedc_ba98_7654_3210,
                0x9abc_def0_1234_5678,
                0x7654_3210_fedc_ba98,
            ]),
        }
    }
}

#[inline(never)]
fn raiju(h: &Hashes, msg: &[u8]) -> u64 {
    h.raiju
        .hash_avalanche(black_box(msg), 0x5a5a_a5a5_0123_4567) as u64
}

#[inline(never)]
fn sip(h: &Hashes, msg: &[u8]) -> u64 {
    let mut state = SipHasher24::new_with_keys(h.sip.0, h.sip.1);
    state.write(black_box(msg));
    state.finish()
}

#[inline(never)]
fn highway(h: &Hashes, msg: &[u8]) -> u64 {
    let mut state = HighwayHasher::new(h.highway);
    state.append(black_box(msg));
    state.finalize64()
}

fn call(h: &Hashes, which: usize, msg: &[u8]) {
    let output = match which {
        0 => raiju(h, msg),
        1 => sip(h, msg),
        _ => highway(h, msg),
    };
    black_box(output);
}

fn time_ns(f: &mut dyn FnMut(), target: f64, rounds: usize) -> f64 {
    let t = Instant::now();
    let mut iters = 0u64;
    while t.elapsed().as_secs_f64() < target {
        f();
        iters += 1;
    }
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e9 / iters as f64);
    }
    best
}

fn buffer(len: usize) -> (Vec<u8>, usize) {
    let mut buf: Vec<u8> = (0..len + 64).map(|i| (i * 131 + 7) as u8).collect();
    let off = (64 - buf.as_ptr() as usize % 64) % 64;
    buf.truncate(off + len);
    (buf, off)
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let target = if quick { 0.005 } else { 0.02 };
    let rounds = if quick { 3 } else { 5 };
    let passes = if quick { 1 } else { 3 };
    let h = Hashes::new();

    println!("architecture: {}", std::env::consts::ARCH);
    println!("RaijuHash backend: {:?}", h.raiju.backend());
    println!("RaijuHash output: low 64 bits of hash_avalanche with a fixed tweak");
    println!("| bytes | {} |", NAMES.join(" | "));
    println!("|---:|---:|---:|---:|");

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
        262144,
        1 << 20,
        16 << 20,
        64 << 20,
    ];
    for len in sizes {
        let (buf, off) = buffer(len);
        let msg = &buf[off..off + len];
        let mut best = [f64::INFINITY; 3];
        for _ in 0..passes {
            for (i, b) in best.iter_mut().enumerate() {
                *b = b.min(time_ns(&mut || call(&h, i, msg), target, rounds));
            }
        }
        let cells: Vec<String> = best
            .iter()
            .map(|&ns| {
                if len >= 1024 {
                    format!("{ns:.1} ({:.1})", len as f64 / ns / 1e0)
                } else {
                    format!("{ns:.1}")
                }
            })
            .collect();
        println!("| {len} | {} |", cells.join(" | "));
    }

    println!("\nBulk cells are ns/hash (GB/s); shorter cells are ns/hash.");
}

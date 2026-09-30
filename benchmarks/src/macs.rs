//! Full message-authentication comparison on one prepared key.
//!
//! The message is authenticated as data for every construction. The nonce is
//! unique per invocation: RaijuHash, GMAC, ChaCha20-Poly1305 and
//! UMAC use their native nonce input; HMAC authenticates the nonce as a
//! 16-byte message prefix. The AEADs authenticate the message as AAD with an
//! empty plaintext, so these timings measure their complete tag path without
//! encrypting the message payload.
//!
//! Usage: `macs [--quick]`. Reports per-tag latency and bulk throughput.

use std::hint::black_box;
use std::time::Instant;

use aes_gcm::{Aes128Gcm, KeyInit as _, aead::AeadInPlace as _};
use chacha20poly1305::ChaCha20Poly1305;
use hmac::{Hmac, Mac as _};
use purecrypto::mac::Umac128;
use raijuhash::Mac as RaijuMac;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const KEY16: [u8; 16] = [0x42; 16];
const KEY32: [u8; 32] = [0x24; 32];
const NONCE_START: u64 = 1;

struct Macs {
    raiju: RaijuMac,
    hmac: HmacSha256,
    gmac: Aes128Gcm,
    chacha: ChaCha20Poly1305,
    umac: Umac128,
}

impl Macs {
    fn new() -> Self {
        Self {
            raiju: RaijuMac::from_seed(0x0123_4567_89ab_cdef),
            hmac: <HmacSha256 as hmac::Mac>::new_from_slice(&KEY32).unwrap(),
            gmac: Aes128Gcm::new_from_slice(&KEY16).unwrap(),
            chacha: ChaCha20Poly1305::new_from_slice(&KEY32).unwrap(),
            umac: Umac128::new(&KEY16),
        }
    }
}

#[derive(Clone, Copy)]
enum Which {
    Raiju,
    Hmac,
    Gmac,
    ChaChaPoly,
    Umac,
}

const ALGORITHMS: [(&str, Which); 5] = [
    ("RaijuHash-Mac", Which::Raiju),
    ("HMAC-SHA256-128", Which::Hmac),
    ("AES-128-GMAC", Which::Gmac),
    ("ChaCha20-Poly1305", Which::ChaChaPoly),
    ("UMAC-AES-128", Which::Umac),
];

#[inline(never)]
fn tag(macs: &Macs, which: Which, msg: &[u8], nonce_counter: &mut u64) {
    let n = *nonce_counter;
    *nonce_counter = nonce_counter.wrapping_add(1);
    let nonce128 = n as u128;
    let nonce64 = n.to_le_bytes();
    match which {
        Which::Raiju => {
            black_box(macs.raiju.tag(nonce128, black_box(msg)));
        }
        Which::Hmac => {
            let mut h = macs.hmac.clone();
            h.update(&nonce128.to_le_bytes());
            h.update(black_box(msg));
            let out = h.finalize().into_bytes();
            // Match the 128-bit tag size of the other constructions.
            black_box(&out[..16]);
        }
        Which::Gmac => {
            let nonce_bytes = nonce96(n);
            let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
            let mut empty = [];
            let out = macs
                .gmac
                .encrypt_in_place_detached(nonce, black_box(msg), &mut empty)
                .unwrap();
            black_box(out);
        }
        Which::ChaChaPoly => {
            let nonce_bytes = nonce96(n);
            let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);
            let mut empty = [];
            let out = macs
                .chacha
                .encrypt_in_place_detached(nonce, black_box(msg), &mut empty)
                .unwrap();
            black_box(out);
        }
        Which::Umac => {
            let mut h = macs.umac.clone();
            h.update(black_box(msg));
            black_box(h.finalize(&nonce64));
        }
    }
}

fn nonce96(n: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&n.to_le_bytes());
    nonce
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
    let target = if quick { 0.003 } else { 0.02 };
    let rounds = if quick { 3 } else { 5 };
    let passes = if quick { 1 } else { 3 };
    let macs = Macs::new();

    println!("architecture: {}", std::env::consts::ARCH);
    println!(
        "RaijuHash backend: {:?}",
        raijuhash::Key::from_seed(1).backend()
    );
    println!("nonce: monotonically increasing, unique within each algorithm run");
    println!("tag size: 128 bits (HMAC-SHA256 truncated to 128 bits)");
    println!("AEAD comparison: message in AAD, empty plaintext (authentication only)");
    println!("\n| bytes | {} |", ALGORITHMS.map(|x| x.0).join(" | "));
    println!("|---:|---:|---:|---:|---:|---:|");

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
        8192,
        16384,
        65536,
        262144,
        1 << 20,
        4 << 20,
        16 << 20,
    ];
    let mut nonce_counters = [NONCE_START; ALGORITHMS.len()];
    for len in sizes {
        let (buf, off) = buffer(len);
        let msg = &buf[off..off + len];
        let mut best = [f64::INFINITY; ALGORITHMS.len()];
        for _ in 0..passes {
            for (i, (_, which)) in ALGORITHMS.iter().enumerate() {
                let mut nonce = nonce_counters[i];
                let mut f = || tag(&macs, *which, msg, &mut nonce);
                best[i] = best[i].min(time_ns(&mut f, target, rounds));
                nonce_counters[i] = nonce;
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

    println!("\nBulk throughput cells are GB/s; shorter-size cells are ns/tag.");
}

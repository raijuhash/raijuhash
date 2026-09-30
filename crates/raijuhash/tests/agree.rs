//! Every backend and the streaming API must agree with the reference model.

use raijuhash::{Backend, KEY_BYTES, Key, Params, reference};

fn bytes(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed ^ 0x9e37_79b9_7f4a_7c15;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect()
}

fn key_bytes(seed: u64) -> Box<[u8; KEY_BYTES]> {
    bytes(seed, KEY_BYTES).into_boxed_slice().try_into().unwrap()
}

fn backends() -> Vec<Backend> {
    let all = [
        Backend::Portable,
        Backend::NeonEor3,
        Backend::NeonPlain,
        Backend::X86Sse,
        Backend::X86Avx2,
        Backend::X86Avx512,
        #[cfg(feature = "emulated-vpclmul")]
        Backend::X86Avx2Emulated,
    ];
    let found: Vec<Backend> = all.into_iter().filter(|b| b.supported()).collect();
    eprintln!("testing backends: {found:?}");
    found
}

/// Lengths around every structural boundary.
fn lengths() -> Vec<usize> {
    let mut v: Vec<usize> = (0..=1100).collect();
    for base in [2048usize, 4096, 8192, 8192 * 2, 8192 * 3] {
        for d in [-129i64, -128, -127, -1, 0, 1, 31, 127, 128, 129] {
            let n = base as i64 + d;
            if n > 0 {
                v.push(n as usize);
            }
        }
    }
    v.extend([8192 * 5 + 777, 100_003]);
    v.sort();
    v.dedup();
    v
}

#[test]
fn backends_match_reference() {
    for seed in 0..2u64 {
        let kb = key_bytes(seed);
        let params = Params::from_bytes(&kb);
        let keys: Vec<Key> = backends().into_iter().map(|b| Key::with_backend(&kb, b)).collect();
        let msg = bytes(seed + 100, 100_003);
        for n in lengths() {
            let m = &msg[..n];
            // The bit-at-a-time reference is slow; check it on a subset.
            let want = if n <= 1100 || n % 64 != 17 {
                reference::hash(&params, m)
            } else {
                keys[0].hash(m)
            };
            for k in &keys {
                assert_eq!(k.hash(m), want, "len {n} backend {:?}", k.backend());
            }
        }
    }
}

#[test]
fn streaming_matches_oneshot() {
    let kb = key_bytes(7);
    let msg = bytes(8, 40_000);
    for b in backends() {
        let key = Key::with_backend(&kb, b);
        for n in [0usize, 1, 15, 16, 31, 32, 127, 128, 129, 1000, 8191, 8192, 8193, 20000, 40000] {
            let m = &msg[..n];
            let want = key.hash(m);
            // Fixed-size pieces.
            for piece in [1usize, 7, 16, 31, 127, 128, 129, 1024, 1500, 2048, 4095, 8192, 9000] {
                let mut h = key.hasher();
                for c in m.chunks(piece) {
                    h.update(c);
                }
                assert_eq!(h.finalize(), want, "len {n} piece {piece} {b:?}");
            }
            // Every two-way split for short messages.
            if n <= 300 {
                for s in 0..=n {
                    let mut h = key.hasher();
                    h.update(&m[..s]);
                    h.update(&[]);
                    h.update(&m[s..]);
                    assert_eq!(h.finalize(), want, "len {n} split {s} {b:?}");
                }
            }
        }
    }
}

#[test]
fn finalize_is_repeatable_and_reset_works() {
    let key = Key::from_entropy(&key_bytes(3));
    let msg = bytes(4, 10_000);
    let mut h = key.hasher();
    h.update(&msg[..5000]);
    let mid = h.finalize();
    assert_eq!(mid, key.hash(&msg[..5000]));
    assert_eq!(h.finalize(), mid);
    h.update(&msg[5000..]);
    assert_eq!(h.finalize(), key.hash(&msg));
    h.reset();
    assert_eq!(h.finalize(), key.hash(&[]));
}

#[test]
fn field_arithmetic_identities() {
    // x^128 reduces to x^7 + x^2 + x + 1.
    let x64 = 1u128 << 64;
    assert_eq!(reference::gf_mul(x64, x64), 0x87);
    let a = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210u128;
    let b = 0x0f1e_2d3c_4b5a_6978_8796_a5b4_c3d2_e1f0u128;
    assert_eq!(reference::gf_mul(a, b), reference::gf_mul(b, a));
    assert_eq!(reference::gf_mul(a, 1), a);
    // Distributivity.
    let c = 0xdead_beef_u128 << 70 | 12345;
    assert_eq!(reference::gf_mul(a, b ^ c), reference::gf_mul(a, b) ^ reference::gf_mul(a, c));
}

#[test]
fn distinct_inputs_differ() {
    // Sanity only: this cannot test the universality bound.
    let key = Key::from_entropy(&key_bytes(11));
    let mut seen = std::collections::HashSet::new();
    for n in 0..300 {
        assert!(seen.insert(key.hash(&vec![0u8; n])), "zero messages of length {n} collided");
    }
    let base = bytes(12, 9000);
    for i in (0..9000).step_by(97) {
        let mut m = base.clone();
        m[i] ^= 1;
        assert!(seen.insert(key.hash(&m)));
    }
}

#[cfg(feature = "aes")]
#[test]
fn mac_roundtrip_and_domains() {
    let mac = raijuhash::Mac::from_seed(42);
    let msg = bytes(9, 5000);
    let t = mac.tag(7, &msg);
    assert!(mac.verify(7, &msg, t));
    assert!(!mac.verify(8, &msg, t));
    let mut m2 = msg.clone();
    m2[100] ^= 1;
    assert!(!mac.verify(7, &m2, t));
    let mut h = mac.hasher();
    for c in msg.chunks(333) {
        h.update(c);
    }
    assert_eq!(h.finalize(7), t);
    assert!(h.verify(7, t));
    // Every tag bit is compared.
    for bit in 0..128 {
        assert!(!mac.verify(7, &msg, t ^ 1 << bit), "bit {bit}");
        assert!(!h.verify(7, t ^ 1 << bit), "bit {bit}");
    }
    // The nonce's top bit is ignored by design.
    assert_eq!(mac.tag(7 | 1 << 127, &msg), t);
    // Raw hashing with the same seed uses an unrelated key.
    let key = Key::from_seed(42);
    assert_ne!(key.hash(&msg), t);
}

#[cfg(feature = "std")]
#[test]
fn parallel_matches_serial() {
    let kb = key_bytes(21);
    // `hash_parallel` uses one thread per 2 MiB: the last two lengths take
    // two and three (with a partial last task).
    let big = (6 << 20) + 8192 * 5 + 777;
    let msg = bytes(22, big);
    for b in backends() {
        let key = Key::with_backend(&kb, b);
        for n in [0usize, 100, 8192 * 31, 8192 * 32, 8192 * 33 + 5, 8192 * 48, 8192 * 70 + 3000, (4 << 20) + 3000, big] {
            let m = &msg[..n];
            let want = key.hash(m);
            for threads in [1usize, 2, 3, 8] {
                assert_eq!(key.hash_parallel(m, threads), want, "len {n} threads {threads} {b:?}");
            }
        }
    }
}

/// One-shot hashing of many chunks takes the two-chunk kernels (chosen by
/// size: L1, L2 and memory variants); streaming in pieces smaller than a
/// chunk takes the group kernels instead, so the two paths check each other.
/// The portable backend checks the moderate sizes too.
#[test]
fn bulk_kernels_match_streaming() {
    let kb = key_bytes(31);
    let big = bytes(32, 8192 * 1283 + 555);
    let chunks = [2usize, 3, 16, 17, 18, 33, 41, 1279, 1280, 1281, 1283];
    for b in backends() {
        let key = Key::with_backend(&kb, b);
        let portable = Key::with_backend(&kb, Backend::Portable);
        for &c in &chunks {
            for tail in [0usize, 1, 555] {
                let m = &big[..8192 * c + tail];
                let want = key.hash(m);
                let mut h = key.hasher();
                for piece in m.chunks(1000) {
                    h.update(piece);
                }
                assert_eq!(h.finalize(), want, "{c} chunks + {tail} {b:?}");
                if c < 64 {
                    assert_eq!(portable.hash(m), want, "{c} chunks + {tail} {b:?} vs portable");
                }
            }
        }
    }
}

/// A seeded key generates table rows on first use; threads racing on a
/// fresh key must all see complete rows.
#[cfg(all(feature = "aes", feature = "std"))]
#[test]
fn lazy_rows_under_contention() {
    let lengths = [0usize, 31, 64, 129, 700, 1000, 1025, 5000, 8191, 8192, 20_000, 70_000];
    let msg = bytes(77, 70_000);
    let want: Vec<u128> = {
        let key = Key::from_seed(1234);
        lengths.iter().map(|&n| key.hash(&msg[..n])).collect()
    };
    for round in 0..20 {
        let key = Key::from_seed(1234);
        std::thread::scope(|s| {
            for t in 0..8 {
                let (key, msg, want) = (&key, &msg, &want);
                s.spawn(move || {
                    for i in 0..lengths.len() {
                        let j = (i + t + round) % lengths.len();
                        assert_eq!(key.hash(&msg[..lengths[j]]), want[j], "len {} thread {t}", lengths[j]);
                    }
                });
            }
        });
        // A clone of a partly generated key is complete and equal.
        let fresh = Key::from_seed(1234);
        fresh.hash(&msg[..129]);
        assert_eq!(fresh.clone().hash(&msg[..20_000]), want[10]);
    }
}

#[test]
fn avalanche_matches_reference() {
    let kb = key_bytes(41);
    let p = Params::from_bytes(&kb);
    let msg = bytes(42, 20_000);
    for b in backends() {
        let key = Key::with_backend(&kb, b);
        for n in [0usize, 1, 15, 31, 32, 64, 65, 1000, 1024, 8193, 20_000] {
            for tweak in [0u64, 1, u64::MAX, 0x1234_5678] {
                let m = &msg[..n];
                let want = reference::hash_avalanche(&p, m, tweak);
                assert_eq!(key.hash_avalanche(m, tweak), want, "len {n} tweak {tweak} {b:?}");
                let mut h = key.hasher();
                h.update(m);
                assert_eq!(h.finalize_avalanche(tweak), want, "streaming len {n} {b:?}");
            }
        }
    }
    // The mix is a bijection: distinct inputs map to distinct outputs, and
    // it is not the identity.
    for x in [0u128, 1, u128::MAX, 1 << 64, 0xdead_beef] {
        assert_ne!(reference::avalanche_mix(x, 0), reference::avalanche_mix(x ^ 1, 0));
        assert_ne!(reference::avalanche_mix(x, 0), reference::avalanche_mix(x, 1));
    }
}

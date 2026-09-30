//! Frozen outputs pinning version 1 of the definition (SPEC.md). Any change
//! here is a breaking change of the hash function.

use raijuhash::{Backend, KEY_BYTES, Key};

fn msg(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 + 7) as u8).collect()
}

/// Key bytes `i * 13 + 5`, message bytes `i * 31 + 7`; generated with the
/// reference model.
const PATTERN_KEY: &[(usize, u128)] = &[
    (0, 0x584b3e3124170afdf0e3d6c9bcafa295),
    (1, 0x000fc20c256f04ee23135fe801794877),
    (15, 0xebe17fc2bb4337e85308a1d43247f806),
    (16, 0x580381ccfee1c42a09f636f9f6db0930),
    (31, 0x7b4d70fca6edb0581a539e1a0a5be81c),
    (32, 0xb94d7ba165e7bc46267fd5e0b6e012eb),
    (127, 0x6d1aaf8c35ae00242f969fb2c9ac2a4c),
    (128, 0xf5413437937f6e3440c3fddd38b731e1),
    (129, 0x134d99c0c09f516c013b1f061675b86b),
    (1000, 0xc3ee0a9c543d9978dd14c2160b740767),
    (1024, 0xe6821818e4b3f51319f4e6d806acf113),
    (8191, 0x89deeac678cfd9391b1b54d045ba6fdf),
    (8192, 0x9e6b50b1cf5b185edef367c5708e142c),
    (8193, 0xc95a64f89b205ecd069afcd0c347f38d),
    (20000, 0x1cd9404a351f6d9d5c92c467dd0a3ac3),
];

/// `Key::hash_avalanche` with the pattern key, tweaks 0 and 42; generated
/// with the reference model.
const AVALANCHE: &[(usize, u128, u128)] = &[
    (0, 0x36e2e7a7ae21902507ebbdb6187ddac2, 0xb18595818602b216705fb078c37260c7),
    (31, 0x97b4207c5e328ff7847b3f606390a4f2, 0x5c2a1cae6d095781dcf0e6c11f1b08e6),
    (32, 0xa81f33cad663c37850bc60d726288bdb, 0xe37b6a3fc2369063649be63b92f90f2e),
    (1000, 0xd7a94828f3234b17c217f632f8782a45, 0x2664eec0f59b1f0cb7f4cd11179e7ec3),
    (8193, 0x375583146db445caf01f5cb521b98df7, 0xba894773e9727a3dc7687e45e110448c),
];

/// `Key::from_seed(0x0123456789abcdeffedcba9876543210)`, which also pins the
/// AES-128 counter-mode key derivation.
#[cfg(feature = "aes")]
const SEED_KEY: &[(usize, u128)] = &[
    (0, 0xfefe20e285b50b76f565a42ddf271404),
    (1, 0x89293b117bb9018a50c16a14d91d4f15),
    (15, 0xcc9591b916dfc6d324902f70a0a62131),
    (16, 0x11d5536198d9f9d327edc8c663d6d341),
    (31, 0xb717f64685896899108cb2693901192e),
    (32, 0xabd3a3d633e3cfd1fc53a9e8c989aa99),
    (127, 0xdb9ebda78bcf3e3f555b7d56ef5c9160),
    (128, 0xef402c49e3f24723b6ac544c7cd9cd54),
    (129, 0x01b76e36e5b1033b931fca4cde7e0827),
    (1000, 0x50ad8def3a2ad884dc98f38b9b739ada),
    (1024, 0xbee9da316751e6ee180a9c7a7f6f7a40),
    (8191, 0x2cbfa1832d654b9b7e2c26dff09a754c),
    (8192, 0xa5d128ebe151434e55316fd45915b6ce),
    (8193, 0xbec2ea94b5981e89baedc24fb6e89b0c),
    (20000, 0xaf96b9cfd077ae799e3ace9a34bcf29b),
];

/// `Mac::from_seed(same seed)` with nonce 42.
#[cfg(feature = "aes")]
const MAC: &[(usize, u128)] = &[
    (0, 0xbca78e50301702005c6a02fefaf0a8b8),
    (100, 0x60d0721a985bf38ffa340b9013447781),
    (5000, 0xc9aae93790c406ff3948e8da59202160),
];

#[cfg(feature = "aes")]
const SEED: u128 = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210;

fn all_backends() -> impl Iterator<Item = Backend> {
    [Backend::Portable, Backend::NeonEor3, Backend::NeonPlain, Backend::X86Sse, Backend::X86Avx2, Backend::X86Avx512]
        .into_iter()
        .filter(|b| b.supported())
}

#[test]
fn pattern_key_vectors() {
    let kb: Vec<u8> = (0..KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
    let kb: &[u8; KEY_BYTES] = kb.as_slice().try_into().unwrap();
    for b in all_backends() {
        let key = Key::with_backend(kb, b);
        for &(n, want) in PATTERN_KEY {
            assert_eq!(key.hash(&msg(n)), want, "len {n} backend {b:?}");
        }
        for &(n, t0, t42) in AVALANCHE {
            assert_eq!(key.hash_avalanche(&msg(n), 0), t0, "avalanche len {n} backend {b:?}");
            assert_eq!(key.hash_avalanche(&msg(n), 42), t42, "avalanche len {n} backend {b:?}");
        }
    }
}

#[cfg(feature = "aes")]
#[test]
fn seed_key_vectors() {
    let key = Key::from_seed(SEED);
    for &(n, want) in SEED_KEY {
        assert_eq!(key.hash(&msg(n)), want, "len {n}");
    }
}

#[cfg(feature = "aes")]
#[test]
fn mac_vectors() {
    let mac = raijuhash::Mac::from_seed(SEED);
    for &(n, want) in MAC {
        assert_eq!(mac.tag(42, &msg(n)), want, "len {n}");
    }
}

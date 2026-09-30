//! Print frozen test vectors (used to generate tests/vectors.rs).
use raijuhash::{KEY_BYTES, Key, Params, reference};

fn msg(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 + 7) as u8).collect()
}

fn main() {
    let lens = [0usize, 1, 15, 16, 31, 32, 127, 128, 129, 1000, 1024, 8191, 8192, 8193, 20000];
    let kb: Vec<u8> = (0..KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
    let kb: &[u8; KEY_BYTES] = kb.as_slice().try_into().unwrap();
    let p = Params::from_bytes(kb);
    println!("// Key bytes i*13+5, message bytes i*31+7, via the reference model.");
    for n in lens {
        println!("    ({n}, 0x{:032x}),", reference::hash(&p, &msg(n)));
    }
    let key = Key::from_seed(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210);
    println!("// Key::from_seed(0x0123456789abcdeffedcba9876543210).");
    for n in lens {
        println!("    ({n}, 0x{:032x}),", key.hash(&msg(n)));
    }
    println!("// Avalanche finalizer, pattern key, tweaks 0 and 42, via the reference model.");
    for n in [0usize, 31, 32, 1000, 8193] {
        let (a, b) = (reference::hash_avalanche(&p, &msg(n), 0), reference::hash_avalanche(&p, &msg(n), 42));
        println!("    ({n}, 0x{a:032x}, 0x{b:032x}),");
    }
    let mac = raijuhash::Mac::from_seed(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210);
    println!("// Mac::from_seed(same), nonce 42.");
    for n in [0usize, 100, 5000] {
        println!("    ({n}, 0x{:032x}),", mac.tag(42, &msg(n)));
    }
}

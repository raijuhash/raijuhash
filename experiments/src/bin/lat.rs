//! Latency and throughput of `Key::hash` for short messages: a dependent
//! chain (each input byte 0 depends on the previous output) and independent
//! back-to-back calls. Usage: lat [sizes...]
use std::hint::black_box;
use std::time::Instant;

fn best(f: &mut dyn FnMut(), n: u64) -> f64 {
    let mut b = f64::INFINITY;
    for _ in 0..15 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        b = b.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
    }
    b
}

fn main() {
    // `RJ_BACKEND=sse|avx2` forces a backend (with a pattern key).
    let key = match std::env::var("RJ_BACKEND").as_deref() {
        Ok(name) => {
            let backend = match name {
                "sse" => raijuhash::Backend::X86Sse,
                "avx2" => raijuhash::Backend::X86Avx2,
                _ => raijuhash::Backend::X86Avx512,
            };
            let bytes: Vec<u8> = (0..raijuhash::KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
            raijuhash::Key::with_backend(bytes.as_slice().try_into().unwrap(), backend)
        },
        Err(_) => raijuhash::Key::from_seed(7),
    };
    let mac = raijuhash::Mac::from_seed(7);
    let sizes: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    println!("{:>6} {:>9} {:>9} {:>9} {:>9} {:>9}", "bytes", "lat ns", "addr lat", "thr ns", "mac lat", "mac thr");
    for sz in sizes {
        let mut m: Vec<u8> = (0..sz.max(1)).map(|i| i as u8).collect();
        m.truncate(sz);
        let n = 2_000_000;
        let lat = best(&mut || { let h = key.hash(&m); if let Some(b) = m.first_mut() { *b ^= h as u8; } else { black_box(h); } }, n);
        // Dependency through the address: two copies, picked by the last hash.
        let two: Vec<u8> = m.iter().chain(m.iter()).copied().chain(std::iter::repeat(0).take(64)).collect();
        let mut h = 0u128;
        let alat = best(&mut || { let o = (h as usize & 1) * sz; h = key.hash(&two[o..o + sz]); }, n);
        let thr = best(&mut || { black_box(key.hash(black_box(&m))); }, n);
        let mut nonce = 1u128;
        let mlat = best(&mut || { let t = mac.tag(nonce, &m); nonce = nonce.wrapping_add(t | 1); }, n);
        let mthr = best(&mut || { nonce += 1; black_box(mac.tag(nonce, black_box(&m))); }, n);
        println!("{sz:>6} {lat:>9.2} {alat:>9.2} {thr:>9.2} {mlat:>9.2} {mthr:>9.2}");
    }
}

//! `Key::hash` / `Mac::tag` throughput over randomly mixed lengths (so the
//! length branches mispredict as in real traffic), and a dependent chain
//! over the same mix. The sequence is long (2^18 calls) so that the branch
//! predictor cannot learn it. Usage: mixlen [lo-hi ...], e.g. mixlen 1-15 1-64
use std::hint::black_box;
use std::time::Instant;

fn best(f: &mut dyn FnMut(), n: u64) -> f64 {
    let mut b = f64::INFINITY;
    for _ in 0..5 {
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
    let buf: Vec<u8> = (0..65536 + 4096).map(|i| (i * 7 + 3) as u8).collect();
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    println!("{:>8} {:>9} {:>9} {:>9}", "lengths", "thr ns", "chain ns", "mac thr");
    let only_mac = std::env::var("MAC_ONLY").is_ok();
    for arg in std::env::args().skip(1) {
        let (lo, hi) = arg.split_once('-').map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap())).unwrap();
        let cases: Vec<(u16, u16)> =
            (0..1 << 18).map(|_| ((next() % 65536) as u16, (lo + (next() % (hi - lo + 1) as u64) as usize) as u16)).collect();
        let cases: Vec<(usize, usize)> = cases.iter().map(|&(o, l)| (o as usize, l as usize)).collect();
        let n = cases.len() as u64;
        if only_mac {
            let mut nonce = 1u128;
            let t = best(&mut || { for &(o, l) in &cases { nonce += 1; black_box(mac.tag(nonce, black_box(&buf[o..o + l]))); } }, 3) / n as f64;
            println!("{arg:>8} mac thr {t:.2}");
            continue;
        }
        let thr = best(&mut || { for &(o, l) in &cases { black_box(key.hash(black_box(&buf[o..o + l]))); } }, 1) / n as f64;
        let mut h = 0u128;
        let chain = best(&mut || { for &(o, l) in &cases { let o = o ^ (h as usize & 1); h = key.hash(&buf[o..o + l]); } }, 1) / n as f64;
        let mut nonce = 1u128;
        let mthr = best(&mut || { for &(o, l) in &cases { nonce += 1; black_box(mac.tag(nonce, black_box(&buf[o..o + l]))); } }, 1) / n as f64;
        println!("{arg:>8} {thr:>9.2} {chain:>9.2} {mthr:>9.2}");
    }
}

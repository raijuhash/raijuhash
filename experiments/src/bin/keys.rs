//! Many active keys (CANDIDATES X11): `Key::hash` / `Mac::tag` of one
//! fixed-length message under `n` keys taken in a shuffled order, so every
//! call finds its key's fields wherever the cache left them.
//! Usage: keys [lengths...]
use std::hint::black_box;
use std::time::Instant;

fn best(f: &mut dyn FnMut()) -> f64 {
    f();
    let mut b = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        f();
        b = b.min(t.elapsed().as_secs_f64());
    }
    b
}

fn main() {
    let lens: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let counts = [1usize, 64, 1024, 4096];
    let max = *counts.iter().max().unwrap();
    let keys: Vec<raijuhash::Key> = (0..max as u128).map(raijuhash::Key::from_seed).collect();
    let macs: Vec<raijuhash::Mac> = (0..max as u128).map(raijuhash::Mac::from_seed).collect();
    let msg: Vec<u8> = (0..4096).map(|i| i as u8).collect();
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    println!("{:>6} {:>6} {:>10} {:>10}", "bytes", "keys", "hash ns", "tag ns");
    for &len in &lens {
        for &n in &counts {
            // A shuffled sequence of key indices, long enough to cycle the cache.
            let order: Vec<usize> = (0..1 << 16)
                .map(|_| {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    (s % n as u64) as usize
                })
                .collect();
            let m = &msg[..len];
            let h = best(&mut || for &i in &order { black_box(keys[i].hash(black_box(m))); }) * 1e9 / order.len() as f64;
            let mut nonce = 1u128;
            let t = best(&mut || for &i in &order { nonce += 1; black_box(macs[i].tag(nonce, black_box(m))); }) * 1e9 / order.len() as f64;
            println!("{len:>6} {n:>6} {h:>10.2} {t:>10.2}");
        }
    }
}

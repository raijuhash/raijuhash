//! Public-API operations for `perf stat`:
//! one <hasher|oneshot|mac|av|reset1|reset2|setup> <bytes> <iters>
use std::hint::black_box;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mode, sz, iters): (&str, usize, u64) =
        (&a[1], a[2].parse().unwrap(), a[3].parse().unwrap());
    let mut buf = vec![0u8; sz + 16384];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = i as u8;
    }
    let off = (4096usize.wrapping_sub(buf.as_ptr() as usize)) % 16384;
    let data = &buf[off..off + sz];
    let ckb = Box::new(raijuhash::Key::from_seed(0x1234_5678_9abc_def0));
    let cks = raijuhash::Key::from_seed(0x1234_5678_9abc_def0);
    let ck: &raijuhash::Key = if std::env::var("HEAP").is_ok() {
        &ckb
    } else {
        &cks
    };

    let cm = raijuhash::Mac::from_seed(0x1234_5678_9abc_def0);
    let mut hs = [ck.hasher(), ck.hasher()];
    let t = std::time::Instant::now();
    for it in 0..iters {
        match mode {
            "hasher" => {
                let mut h = ck.hasher();
                h.update(black_box(data));
                black_box(h.finalize());
            }
            "oneshot" => {
                black_box(ck.hash(black_box(data)));
            }
            "mac" => {
                let mut h = cm.hasher();
                h.update(black_box(data));
                black_box(h.finalize(it as u128));
            }
            "reset1" => {
                let h = &mut hs[0];
                h.reset();
                h.update(black_box(data));
                black_box(h.finalize());
            }
            "reset2" => {
                let h = &mut hs[(it & 1) as usize];
                h.reset();
                h.update(black_box(data));
                black_box(h.finalize());
            }
            "setup" => {
                black_box(&raijuhash::Key::from_seed(black_box(sz as u128)));
            }
            "av" => {
                let mut h = ck.hasher();
                h.update(black_box(data));
                black_box(h.finalize_avalanche(0));
            }
            _ => panic!("mode"),
        }
    }
    eprintln!(
        "{mode} {sz}: {:.2} ns",
        t.elapsed().as_secs_f64() * 1e9 / iters as f64
    );
}

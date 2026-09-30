//! X10: where key-setup time goes. Times `Key::from_seed` / `Mac::from_seed`
//! with the caller's result slot at varying stack offsets (the constructor
//! builds the key in a local and copies it out), a plain 9.3 KB copy at the
//! same offsets, and a boxed destination.
//! Usage: setup [max_pad_lines]; `KEY_LOOP=n setup` only constructs `n` keys.
use std::hint::black_box;
use std::mem::MaybeUninit;
use std::time::Instant;

use raijuhash::{Key, Mac};

fn time_ns(f: &mut dyn FnMut(), iters: u64) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e9 / iters as f64);
    }
    best
}

/// Run `f` with `pad` extra bytes of stack below the caller.
#[inline(never)]
fn padded<const N: usize>(f: &mut dyn FnMut()) {
    let mut pad = [0u8; N];
    black_box(&mut pad);
    f();
    black_box(&pad);
}

fn at_pad(lines: usize, f: &mut dyn FnMut()) {
    macro_rules! arm {
        ($($n:literal)*) => {
            match lines { $($n => padded::<{ $n * 64 + 1 }>(f),)* _ => unreachable!() }
        };
    }
    arm!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
         32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64)
}

#[inline(never)]
fn key_into_local() {
    let k = Key::from_seed(black_box(7));
    black_box(&k);
}

#[inline(never)]
fn mac_into_local() {
    let k = Mac::from_seed(black_box(7));
    black_box(&k);
}

#[inline(never)]
fn copy_local(src: &MaybeUninit<Key>) {
    let mut d = MaybeUninit::<Key>::uninit();
    unsafe { core::ptr::copy_nonoverlapping(black_box(src).as_ptr(), d.as_mut_ptr(), 1) };
    black_box(&d);
}

fn main() {
    // `KEY_LOOP=n`: only `n` calls of `Key::from_seed` (for `perf`).
    if let Ok(n) = std::env::var("KEY_LOOP") {
        for _ in 0..n.parse::<u64>().unwrap() {
            key_into_local();
        }
        return;
    }
    let max: usize = std::env::args().nth(1).map_or(64, |a| a.parse().unwrap());
    let iters = 200_000;
    let src = Box::new(MaybeUninit::<Key>::zeroed());
    println!("{:>5} {:>8} {:>8} {:>8}", "pad", "key ns", "mac ns", "copy ns");
    for lines in (0..=max).step_by(4) {
        let mut out = [0f64; 3];
        let mut t = |i: usize, g: &mut dyn FnMut()| {
            out[i] = time_ns(&mut || at_pad(lines, g), iters);
        };
        t(0, &mut key_into_local);
        t(1, &mut mac_into_local);
        t(2, &mut || copy_local(&src));
        println!("{:>5} {:>8.1} {:>8.1} {:>8.1}", lines * 64, out[0], out[1], out[2]);
    }
    let boxed = time_ns(&mut || { black_box(Box::new(Key::from_seed(black_box(7)))); }, iters);
    println!("Box::new(Key::from_seed): {boxed:.1} ns");
    // A fresh key for every message: setup plus one hash (or tag).
    let msg: Vec<u8> = (0..8192).map(|i| i as u8).collect();
    println!("{:>6} {:>12} {:>12}", "bytes", "key+hash ns", "mac+tag ns");
    for len in [0usize, 16, 64, 65, 128, 1024, 1025, 8192] {
        let m = &msg[..len];
        let h = time_ns(&mut || { let k = Key::from_seed(black_box(7)); black_box(k.hash(black_box(m))); }, iters / 4);
        let t = time_ns(&mut || { let k = Mac::from_seed(black_box(7)); black_box(k.tag(1, black_box(m))); }, iters / 4);
        println!("{len:>6} {h:>12.1} {t:>12.1}");
    }
}

//! CANDIDATES.md X14: parallel hashing with scoped threads spawned per call
//! (as `Key::hash_parallel`) against a persistent worker pool, on the v1
//! chunk compressor (`h0_outer`). Tasks of 16 chunks (128 KiB) are taken
//! from an atomic counter; results combine in order as `P <- P R^m + P_t`.

#[cfg(target_arch = "x86_64")]
mod run {
    use std::hint::black_box;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use experiments::x86::*;

    const TASK: usize = 16 * 8192;

    fn pow(r: u128, mut e: u64) -> u128 {
        let (mut b, mut acc) = (r, 1u128);
        while e > 0 {
            if e & 1 == 1 {
                acc = unsafe { gf_mul_hw(acc, b) };
            }
            b = unsafe { gf_mul_hw(b, b) };
            e >>= 1;
        }
        acc
    }

    fn combine(keys: &Keys, results: &[u128], total_chunks: usize) -> u128 {
        let tasks = results.len();
        let r = keys.outer.r;
        let full = pow(r, 16);
        let mut p = 0u128;
        for (t, &c) in results.iter().enumerate() {
            let m = if t + 1 == tasks { total_chunks - 16 * t } else { 16 };
            let rm = if m == 16 { full } else { pow(r, m as u64) };
            p = unsafe { gf_mul_hw(p, rm) } ^ c;
        }
        p
    }

    fn spawn_per_call(keys: &Keys, msg: &[u8], threads: usize) -> u128 {
        let tasks = msg.len().div_ceil(TASK);
        let results: Vec<AtomicU128> = (0..tasks).map(|_| AtomicU128::default()).collect();
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..threads.min(tasks) {
                s.spawn(|| loop {
                    let t = next.fetch_add(1, Ordering::Relaxed);
                    if t >= tasks {
                        break;
                    }
                    let part = &msg[t * TASK..((t + 1) * TASK).min(msg.len())];
                    results[t].set(unsafe { h0_outer(keys, part) });
                });
            }
        });
        let rs: Vec<u128> = results.iter().map(|r| r.get()).collect();
        combine(keys, &rs, msg.len() / 8192)
    }

    /// A u128 in two relaxed atomics (each slot is written by one thread and
    /// read after a synchronizing join/barrier).
    #[derive(Default)]
    struct AtomicU128(AtomicU64, AtomicU64);
    impl AtomicU128 {
        fn set(&self, v: u128) {
            self.0.store(v as u64, Ordering::Relaxed);
            self.1.store((v >> 64) as u64, Ordering::Relaxed);
        }
        fn get(&self) -> u128 {
            self.0.load(Ordering::Relaxed) as u128 | (self.1.load(Ordering::Relaxed) as u128) << 64
        }
    }

    /// Persistent workers spinning on a job generation; the caller works too.
    struct Pool {
        shared: Arc<Shared>,
        workers: usize,
    }
    struct Shared {
        generation: AtomicUsize,
        done: AtomicUsize,
        next: AtomicUsize,
        job: std::sync::Mutex<(usize, usize, usize, usize)>, // msg ptr, len, keys ptr, results ptr
        stop: std::sync::atomic::AtomicBool,
    }

    impl Pool {
        fn new(workers: usize) -> Pool {
            let shared = Arc::new(Shared {
                generation: AtomicUsize::new(0),
                done: AtomicUsize::new(0),
                next: AtomicUsize::new(0),
                job: std::sync::Mutex::new((0, 0, 0, 0)),
                stop: std::sync::atomic::AtomicBool::new(false),
            });
            for _ in 0..workers {
                let sh = shared.clone();
                std::thread::spawn(move || {
                    let mut seen = 0;
                    loop {
                        let g = loop {
                            let g = sh.generation.load(Ordering::Acquire);
                            if g != seen || sh.stop.load(Ordering::Relaxed) {
                                break g;
                            }
                            std::hint::spin_loop();
                        };
                        if sh.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        seen = g;
                        let job = *sh.job.lock().unwrap();
                        work(&sh, job);
                        sh.done.fetch_add(1, Ordering::AcqRel);
                    }
                });
            }
            Pool { shared, workers }
        }

        fn hash(&self, keys: &Keys, msg: &[u8]) -> u128 {
            let tasks = msg.len().div_ceil(TASK);
            let results: Vec<AtomicU128> = (0..tasks).map(|_| AtomicU128::default()).collect();
            let sh = &self.shared;
            *sh.job.lock().unwrap() = (msg.as_ptr() as usize, msg.len(), keys as *const Keys as usize, results.as_ptr() as usize);
            sh.next.store(0, Ordering::Relaxed);
            sh.done.store(0, Ordering::Relaxed);
            sh.generation.fetch_add(1, Ordering::AcqRel);
            work(sh, *sh.job.lock().unwrap());
            while sh.done.load(Ordering::Acquire) < self.workers {
                std::hint::spin_loop();
            }
            let rs: Vec<u128> = results.iter().map(|r| r.get()).collect();
            combine(keys, &rs, msg.len() / 8192)
        }
    }

    impl Drop for Pool {
        fn drop(&mut self) {
            self.shared.stop.store(true, Ordering::Relaxed);
        }
    }

    fn work(sh: &Shared, job: (usize, usize, usize, usize)) {
        let (p, len, kp, rp) = job;
        let msg = unsafe { std::slice::from_raw_parts(p as *const u8, len) };
        let keys = unsafe { &*(kp as *const Keys) };
        let tasks = len.div_ceil(TASK);
        let results = unsafe { std::slice::from_raw_parts(rp as *const AtomicU128, tasks) };
        loop {
            let t = sh.next.fetch_add(1, Ordering::Relaxed);
            if t >= tasks {
                break;
            }
            let part = &msg[t * TASK..((t + 1) * TASK).min(len)];
            results[t].set(unsafe { h0_outer(keys, part) });
        }
    }

    fn best(reps: usize, f: &mut dyn FnMut()) -> f64 {
        f();
        let mut b = f64::INFINITY;
        for _ in 0..7 {
            let t = Instant::now();
            for _ in 0..reps {
                f();
            }
            b = b.min(t.elapsed().as_secs_f64() / reps as f64);
        }
        b
    }

    pub fn main() {
        let keys = Keys::new(h_key(0), 3);
        let max = 64 << 20;
        let data: Vec<u8> = (0..max).map(|i| (i * 31 + 7) as u8).collect();
        let sizes = [256usize << 10, 512 << 10, 1 << 20, 2 << 20, 4 << 20, 16 << 20, 64 << 20];
        let reps = |size: usize| ((256 << 20) / size).clamp(3, 2000);
        let g = |size: usize, s: f64| size as f64 / s / 1e9;
        println!("GB/s; v1 chunk compressor, 128 KiB tasks");
        print!("{:>12}", "serial");
        for &size in &sizes {
            let m = &data[..size];
            let s1 = best(reps(size), &mut || { black_box(unsafe { h0_outer(&keys, black_box(m)) }); });
            print!(" {:>7.1}", g(size, s1));
        }
        println!("   (sizes 256K 512K 1M 2M 4M 16M 64M)");
        for t in [2usize, 4, 8] {
            print!("{:>12}", format!("spawn x{t}"));
            for &size in &sizes {
                let m = &data[..size];
                assert_eq!(spawn_per_call(&keys, m, t), unsafe { h0_outer(&keys, m) });
                let s = best(reps(size), &mut || { black_box(spawn_per_call(&keys, black_box(m), t)); });
                print!(" {:>7.1}", g(size, s));
            }
            println!();
            let pool = Pool::new(t - 1);
            print!("{:>12}", format!("pool x{t}"));
            for &size in &sizes {
                let m = &data[..size];
                assert_eq!(pool.hash(&keys, m), unsafe { h0_outer(&keys, m) });
                let s = best(reps(size), &mut || { black_box(pool.hash(&keys, black_box(m))); });
                print!(" {:>7.1}", g(size, s));
            }
            println!();
            drop(pool);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}

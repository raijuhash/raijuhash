//! RaijuHash: a fast 128-bit keyed almost-XOR-universal hash.
//!
//! Each 128-byte block is chain-encoded against its predecessor and hashed
//! with carryless NH; per-position sums are combined once per 8 KiB chunk and
//! chunks are absorbed by a polynomial over GF(2^128). With a uniformly random
//! key independent of the messages, two distinct messages of at most `L`
//! bytes collide in any fixed XOR difference with probability at most
//! `(ceil(L / 8192) + 1) / 2^128`. See `SPEC.md` for the exact construction
//! and the argument.
//!
//! Like any universal hash this is not a public checksum: outputs reveal
//! information about the key, so they must stay secret or be masked.

#![no_std]

#[cfg(feature = "std")]
extern crate std;

#[cfg(all(target_arch = "aarch64", target_endian = "little", feature = "aes"))]
mod aes_neon;
#[cfg(feature = "aes")]
mod cipher;
#[cfg(feature = "aes")]
mod mac;
mod params;
mod portable;
#[doc(hidden)]
pub mod reference;
mod state;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
mod neon;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[doc(hidden)]
pub mod neon_asm;
#[cfg(target_arch = "x86_64")]
mod x86;

#[cfg(feature = "aes")]
pub use mac::{Mac, MacHasher};
pub use params::{BLOCK, CHUNK, KEY_BYTES, SHORT_MAX, TABLE_BYTES};
use params::TABLE_ROWS;
#[doc(hidden)]
pub use params::Params;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
#[cfg(feature = "aes")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicU8, Ordering};

use state::{Core, GROUP, GROUP_BLOCKS, copy_small, partial16};

/// `x * a` in GF(2^128). `a` is key material, so the reduction is selected
/// with a mask rather than a branch.
fn mul_x(a: u128) -> u128 {
    (a << 1) ^ (0x87 & (a >> 127).wrapping_neg())
}

/// Whether the CPU supports AVX2 (compile-time target features, or runtime
/// detection with the `std` feature).
#[cfg(target_arch = "x86_64")]
fn has_avx2() -> bool {
    #[cfg(feature = "std")]
    if std::arch::is_x86_feature_detected!("avx2") {
        return true;
    }
    cfg!(target_feature = "avx2")
}

/// `x^64 * a` in GF(2^128): the high word folds back through
/// `x^128 = x^7 + x^2 + x + 1`, without a second overflow.
#[cfg(target_arch = "x86_64")]
fn mul_x64(a: u128) -> u128 {
    let h = a >> 64;
    (a << 64) ^ h ^ (h << 1) ^ (h << 2) ^ (h << 7)
}

/// State before anything was absorbed.
pub(crate) static FRESH: Core = Core::new();

/// A field element together with its 64-bit halves swapped, for schoolbook
/// products that need both `k.lo * a.hi` and `k.hi * a.lo`.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct FieldKey {
    pub k: u128,
    pub swapped: u128,
}

impl FieldKey {
    fn new(k: u128) -> Self {
        FieldKey { k, swapped: k.rotate_left(64) }
    }
}

/// Implementation selected for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "emulated-vpclmul", allow(
    clippy::manual_non_exhaustive,
    reason = "The hidden variant selects a real emulator backend, not a non-exhaustive sentinel."
))]
pub enum Backend {
    Portable,
    /// AArch64 with PMULL and three-way XOR (FEAT_SHA3).
    NeonEor3,
    /// AArch64 with PMULL only.
    NeonPlain,
    /// x86-64 with SSE4.1 and PCLMULQDQ.
    X86Sse,
    /// x86-64 with AVX2 and 256-bit VPCLMULQDQ.
    X86Avx2,
    /// x86-64 with AVX-512 (F, BW, VL, VBMI2) and 512-bit VPCLMULQDQ.
    X86Avx512,
    /// Test-only: the AVX2 kernel with split 128-bit carryless products.
    #[cfg(feature = "emulated-vpclmul")]
    #[doc(hidden)]
    X86Avx2Emulated,
}

impl Backend {
    /// The fastest implementation this CPU supports.
    pub fn detect() -> Backend {
        let order = [
            Backend::NeonEor3,
            Backend::NeonPlain,
            Backend::X86Avx512,
            Backend::X86Avx2,
            Backend::X86Sse,
        ];
        order.into_iter().find(|b| b.supported()).unwrap_or(Backend::Portable)
    }

    /// Whether this CPU can run the backend (compile-time target features,
    /// or runtime detection with the `std` feature).
    pub fn supported(self) -> bool {
        #[allow(unused_macros)]
        macro_rules! has {
            ($detect:ident: $($f:tt),+) => {{
                #[allow(unused_mut)]
                let mut ok = true $(&& cfg!(target_feature = $f))+;
                #[cfg(feature = "std")]
                if !ok {
                    ok = true $(&& std::arch::$detect!($f))+;
                }
                ok
            }};
        }
        match self {
            Backend::Portable => true,
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            Backend::NeonEor3 => has!(is_aarch64_feature_detected: "neon", "aes", "sha3"),
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            Backend::NeonPlain => has!(is_aarch64_feature_detected: "neon", "aes"),
            #[cfg(target_arch = "x86_64")]
            Backend::X86Sse => has!(is_x86_feature_detected: "ssse3", "sse4.1", "pclmulqdq"),
            #[cfg(target_arch = "x86_64")]
            Backend::X86Avx2 => has!(is_x86_feature_detected: "sse4.1", "pclmulqdq", "avx2", "vpclmulqdq"),
            #[cfg(target_arch = "x86_64")]
            Backend::X86Avx512 => has!(
                is_x86_feature_detected: "sse4.1", "pclmulqdq", "avx2", "bmi2", "avx512f", "avx512bw", "avx512vl",
                "avx512vbmi2", "vpclmulqdq"
            ),
            #[cfg(all(target_arch = "x86_64", feature = "emulated-vpclmul"))]
            Backend::X86Avx2Emulated => has!(is_x86_feature_detected: "sse4.1", "pclmulqdq", "avx2"),
            #[allow(unreachable_patterns)]
            _ => false,
        }
    }
}

// Backend dispatch. Each arm is only reachable after `Backend::supported`.
macro_rules! dispatch {
    ($self:ident, $port:expr, |$m:ident| $simd:expr) => {
        match $self.backend {
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            Backend::NeonEor3 => {
                use neon::eor3 as $m;
                unsafe { $simd }
            },
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            Backend::NeonPlain => {
                use neon::plain as $m;
                unsafe { $simd }
            },
            #[cfg(target_arch = "x86_64")]
            Backend::X86Sse => {
                use x86::sse as $m;
                unsafe { $simd }
            },
            #[cfg(target_arch = "x86_64")]
            Backend::X86Avx2 => {
                use x86::avx2 as $m;
                unsafe { $simd }
            },
            #[cfg(target_arch = "x86_64")]
            Backend::X86Avx512 => {
                use x86::avx512 as $m;
                unsafe { $simd }
            },
            #[cfg(all(target_arch = "x86_64", feature = "emulated-vpclmul"))]
            Backend::X86Avx2Emulated => {
                use x86::avx2emu as $m;
                unsafe { $simd }
            },
            _ => $port,
        }
    };
}

/// A prepared key: the chain key table plus field parameters. About 9 KB.
///
/// A key from `from_seed` generates its table rows lazily: construction makes
/// only the field parameters and the rows one group needs, and the first
/// longer message generates the rest, in order, once. `table` is therefore in an
/// `UnsafeCell`: rows below `lazy.ready` are never written again and are the
/// only ones read; rows at or above it are written only under `lazy.lock`.
/// Until then they are uninitialized, hence `MaybeUninit`; the table is only
/// accessed through raw pointers.
#[repr(C, align(64))]
pub struct Key {
    table: UnsafeCell<MaybeUninit<[u8; TABLE_BYTES]>>,
    // After the table (a multiple of 64 bytes), in cache-line order: `pk`,
    // `sk`, one line with what every short message reads (`backend`, `s`,
    // `t`), then `s2`. A hash of up to 64 bytes then touches two to four
    // adjacent lines, which matters when many keys compete for the cache;
    // on x86-64, `pk` and `s2` stay aligned for their 64-byte loads.
    /// Derived for the x86 backends: `(R, T, R2, 0)` and the same times
    /// `x^64`, for packed products with a one-word reduction.
    #[cfg(target_arch = "x86_64")]
    pub(crate) pk: [u128; 8],
    /// Derived for the x86 short path: `(A, B, x^64 A, x^64 B)`.
    #[cfg(target_arch = "x86_64")]
    pub(crate) sk: [u128; 4],
    backend: Backend,
    /// Whether the CPU has AVX2 (for the SSE backend's short path).
    #[cfg(target_arch = "x86_64")]
    avx2: bool,
    pub(crate) s: u128,
    pub(crate) t: FieldKey,
    /// Derived for 32..=64-byte messages (SIMD backends that use it).
    pub(crate) s2: S2Coeffs,
    /// Derived for the x86 avalanche: `(V, x^64 V)`.
    #[cfg(target_arch = "x86_64")]
    pub(crate) vk: [u128; 2],
    pub(crate) a: FieldKey,
    pub(crate) b: FieldKey,
    /// `r` and `r2` must stay adjacent: the bulk kernel reads both.
    pub(crate) r: FieldKey,
    pub(crate) r2: FieldKey,
    /// Avalanche finalizer multiplier.
    pub(crate) v: FieldKey,
    /// Derived: `R + j R2` for positions `j <= 8`, and `x^i R2` for `i = 1..=5`.
    pub(crate) w: [FieldKey; 9],
    pub(crate) r2x: [FieldKey; 5],
    lazy: Lazy,
}

/// Bytes of input per thread below which `hash_parallel` uses fewer threads
/// (Zen 5: two threads beat one from 4 MiB; below that, thread start-up
/// outweighs the split).
#[cfg(feature = "std")]
const PAR_MIN_BYTES: usize = 1 << 21;

/// Rows a seeded key generates at construction: all a message of at most one
/// group needs (its blocks and end position), so only `absorb`, which runs
/// for longer messages, has to check for missing rows.
const EAGER_ROWS: usize = GROUP_BLOCKS + 1;

const _: () = assert!(TABLE_BYTES.is_multiple_of(64));
// Keep the x86-64 cache layout without constraining portable targets whose
// field alignment may place `s2` at a different offset.
#[cfg(target_arch = "x86_64")]
const _: () = assert!(core::mem::offset_of!(Key, s2).is_multiple_of(64));

/// Lazy generation of the table rows of a seeded key.
struct Lazy {
    /// Rows `..ready` of the table are final.
    ready: AtomicU8,
    /// Held while rows at or above `ready` are written.
    #[cfg(feature = "aes")]
    lock: AtomicBool,
    /// The seed and domain label of a key with rows to generate (the cipher
    /// is rebuilt when generating: the key stays small).
    #[cfg(feature = "aes")]
    source: Option<(u128, u64)>,
}

impl Lazy {
    fn full() -> Lazy {
        Lazy {
            ready: AtomicU8::new(TABLE_ROWS as u8),
            #[cfg(feature = "aes")]
            lock: AtomicBool::new(false),
            #[cfg(feature = "aes")]
            source: None,
        }
    }
}

// SAFETY: shared access only reads rows below `lazy.ready` (published with
// Release, observed with Acquire), and rows at or above it are written by
// one thread at a time under `lazy.lock`; every other field is immutable.
unsafe impl Sync for Key {}

impl Clone for Key {
    fn clone(&self) -> Key {
        // With the table complete nothing writes to `self` any more.
        self.ensure_rows(TABLE_ROWS);
        Key {
            // SAFETY: all rows are final (and a `MaybeUninit` copy would be
            // sound regardless).
            table: UnsafeCell::new(unsafe { *self.table.get() }),
            a: self.a,
            b: self.b,
            r: self.r,
            r2: self.r2,
            t: self.t,
            s: self.s,
            v: self.v,
            w: self.w,
            r2x: self.r2x,
            s2: self.s2,
            #[cfg(target_arch = "x86_64")]
            vk: self.vk,
            #[cfg(target_arch = "x86_64")]
            pk: self.pk,
            #[cfg(target_arch = "x86_64")]
            sk: self.sk,
            backend: self.backend,
            #[cfg(target_arch = "x86_64")]
            avx2: self.avx2,
            lazy: Lazy::full(),
        }
    }
}

/// A 32..=64-byte message is one padded block whose Y row is zero, so its
/// output is affine in the eight X words:
/// `H = C + L T + sum_l X[l] * E[l]` with `E[l] = b0[l] R + b1[l] (R + R2)`
/// and `C = S + sum_l (clmul(a0[l], b0[l]) R + clmul(a1[l], b1[l]) (R + R2))`,
/// where `a_j`, `b_j` are the X and Y words of key row `j`. `E` is stored in
/// pairs, `ka[p] = (E[2p].lo, E[2p+1].hi)` and `kb[p] = (E[2p].hi,
/// E[2p+1].lo)`, so that the low and high carryless products of an X word
/// pair with `ka` and `kb` give every half-product without shuffles.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct S2Coeffs {
    pub ka: [u128; 4],
    pub kb: [u128; 4],
    pub c: u128,
}

/// Largest length of the 32..=64-byte form.
pub(crate) const S2_MAX: usize = 64;

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Key").field("backend", &self.backend).finish_non_exhaustive()
    }
}

impl Key {
    /// Raw key table, for benchmarking the kernels in isolation.
    #[doc(hidden)]
    pub fn table_ptr(&self) -> *const u8 {
        self.ensure_rows(TABLE_ROWS);
        self.rows()
    }

    /// The table; only rows the caller made sure of (`ensure_rows`) may be
    /// read through it, and never through a reference.
    #[inline(always)]
    pub(crate) fn rows(&self) -> *const u8 {
        self.table.get() as *const u8
    }

    /// Make rows `..n` of the table final.
    #[inline(always)]
    fn ensure_rows(&self, n: usize) {
        if (self.lazy.ready.load(Ordering::Acquire) as usize) < n {
            self.generate_rows(n);
        }
    }

    /// Keys are always complete without the `aes` feature.
    #[cfg(not(feature = "aes"))]
    #[cold]
    fn generate_rows(&self, _n: usize) {
        unreachable!("table rows missing")
    }

    #[cfg(feature = "aes")]
    #[cold]
    #[inline(never)]
    fn generate_rows(&self, n: usize) {
        loop {
            if self.lazy.ready.load(Ordering::Acquire) as usize >= n {
                return;
            }
            if self.lazy.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                let ready = self.lazy.ready.load(Ordering::Relaxed) as usize;
                if ready < n {
                    // Whole groups of rows at a time; each row is eight blocks.
                    let target = n.next_multiple_of(GROUP_BLOCKS).min(TABLE_ROWS);
                    // Only seeded keys start with rows missing.
                    let Some((seed, label)) = self.lazy.source else { unreachable!("table rows missing") };
                    // SAFETY: rows `ready..target` are not read until `ready`
                    // is published, and only this thread (holding the lock)
                    // writes them.
                    unsafe {
                        let out = (self.table.get() as *mut u8).add(BLOCK * ready);
                        cipher::Aes::new(seed).ctr(label, (ready * BLOCK / 16) as u64, out, (target - ready) * BLOCK / 16);
                    }
                    self.lazy.ready.store(target as u8, Ordering::Release);
                }
                self.lazy.lock.store(false, Ordering::Release);
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Build a key from `KEY_BYTES` uniformly random bytes.
    pub fn from_entropy(bytes: &[u8; KEY_BYTES]) -> Key {
        Self::with_backend(bytes, Backend::detect())
    }

    /// Like `from_entropy`, forcing a particular implementation.
    ///
    /// # Panics
    /// If the CPU does not support `backend`.
    pub fn with_backend(bytes: &[u8; KEY_BYTES], backend: Backend) -> Key {
        assert!(backend.supported(), "backend {backend:?} unsupported on this CPU");
        let mut key = MaybeUninit::<Key>::uninit();
        Self::init_into(&mut key, bytes, backend);
        // SAFETY: `init_into` initializes the key.
        unsafe { key.assume_init() }
    }

    /// `with_backend` into `key`. Out of line and through a pointer so that
    /// LLVM passes the caller's return slot here instead of copying the 9 KB
    /// key out of a local (a third of setup time).
    #[inline(never)]
    fn init_into(key: &mut MaybeUninit<Key>, bytes: &[u8; KEY_BYTES], backend: Backend) {
        let p = key.as_mut_ptr();
        // SAFETY: `finish_init` writes every field.
        unsafe {
            core::ptr::addr_of_mut!((*p).table).cast::<u8>().copy_from_nonoverlapping(bytes.as_ptr(), TABLE_BYTES);
            Self::finish_init(p, bytes[TABLE_BYTES..].try_into().unwrap(), backend);
            core::ptr::addr_of_mut!((*p).lazy).write(Lazy::full());
        }
    }

    /// Fill every field but `table` from the field-element bytes.
    ///
    /// # Safety
    /// `p` must be valid for writes, its table initialized, and `backend`
    /// supported.
    unsafe fn finish_init(p: *mut Key, f: &[u8; KEY_BYTES - TABLE_BYTES], backend: Backend) {
        let f = |i: usize| u128::from_le_bytes(f[16 * i..16 * i + 16].try_into().unwrap());
        let (r, r2) = (f(2), f(3));
        // Multiples of R2 by small polynomials: products with x^i are shifts.
        let mut r2x_raw = [0u128; 8];
        r2x_raw[0] = r2;
        for i in 1..8 {
            r2x_raw[i] = mul_x(r2x_raw[i - 1]);
        }
        let small = |j: usize| (0..8).filter(|b| j >> b & 1 == 1).fold(0, |a, b| a ^ r2x_raw[b]);
        unsafe {
            core::ptr::addr_of_mut!((*p).w).write(core::array::from_fn(|j| FieldKey::new(r ^ small(j))));
            core::ptr::addr_of_mut!((*p).r2x).write(core::array::from_fn(|i| FieldKey::new(r2x_raw[i + 1])));
            core::ptr::addr_of_mut!((*p).a).write(FieldKey::new(f(0)));
            core::ptr::addr_of_mut!((*p).b).write(FieldKey::new(f(1)));
            core::ptr::addr_of_mut!((*p).r).write(FieldKey::new(r));
            core::ptr::addr_of_mut!((*p).r2).write(FieldKey::new(r2));
            core::ptr::addr_of_mut!((*p).t).write(FieldKey::new(f(4)));
            core::ptr::addr_of_mut!((*p).s).write(f(5));
            core::ptr::addr_of_mut!((*p).v).write(FieldKey::new(f(6)));
            core::ptr::addr_of_mut!((*p).backend).write(backend);
            #[cfg(target_arch = "x86_64")]
            core::ptr::addr_of_mut!((*p).avx2).write(backend != Backend::Portable && has_avx2());
            let s2 = match backend {
                #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
                // SAFETY: the backend is supported; the table was written.
                Backend::NeonEor3 | Backend::NeonPlain => {
                    neon::s2_prepare(core::ptr::addr_of!((*p).table).cast::<u8>(), r, r2, f(5))
                },
                #[cfg(target_arch = "x86_64")]
                // SAFETY: the backend is supported; the table was written.
                Backend::X86Avx512 => {
                    x86::avx512::s2_prepare(core::ptr::addr_of!((*p).table).cast::<u8>(), r, r2, f(5))
                },
                #[cfg(target_arch = "x86_64")]
                // SAFETY: every x86 backend has PCLMULQDQ; the table was written.
                b if b != Backend::Portable => {
                    x86::s2_prepare(core::ptr::addr_of!((*p).table).cast::<u8>(), r, r2, f(5))
                },
                _ => S2Coeffs { ka: [0; 4], kb: [0; 4], c: 0 },
            };
            core::ptr::addr_of_mut!((*p).s2).write(s2);
            #[cfg(target_arch = "x86_64")]
            {
                let t = f(4);
                core::ptr::addr_of_mut!((*p).pk).write([r, t, r2, 0, mul_x64(r), mul_x64(t), mul_x64(r2), 0]);
                core::ptr::addr_of_mut!((*p).sk).write([f(0), f(1), mul_x64(f(0)), mul_x64(f(1))]);
                core::ptr::addr_of_mut!((*p).vk).write([f(6), mul_x64(f(6))]);
            }
        }
    }

    /// Expand a 128-bit secret seed into a key with AES-128 in counter mode.
    ///
    /// The bound then additionally assumes AES-128 is a pseudorandom
    /// permutation. Counter blocks carry a domain label so other uses of the
    /// same seed (such as a MAC) can use disjoint inputs.
    #[cfg(feature = "aes")]
    pub fn from_seed(seed: u128) -> Key {
        Self::expand(seed, &cipher::Aes::new(seed), DOMAIN_KEY)
    }

    /// The key whose `KEY_BYTES` are the counter-mode blocks of `aes` (AES
    /// under `seed`) with `label`. Only the field parameters and the rows a
    /// single group needs (`EAGER_ROWS`) are generated now; see `Key`.
    #[cfg(feature = "aes")]
    pub(crate) fn expand(seed: u128, aes: &cipher::Aes, label: u64) -> Key {
        let mut key = MaybeUninit::<Key>::uninit();
        Self::expand_into(&mut key, seed, aes, label);
        // SAFETY: `expand_into` initializes the key.
        unsafe { key.assume_init() }
    }

    /// `expand` into `key` (out of line for the reason given at `init_into`).
    #[cfg(feature = "aes")]
    #[inline(never)]
    fn expand_into(key: &mut MaybeUninit<Key>, seed: u128, aes: &cipher::Aes, label: u64) {
        let backend = Backend::detect();
        let p = key.as_mut_ptr();
        let mut fields = [0u8; KEY_BYTES - TABLE_BYTES];
        // SAFETY: the first rows are generated straight into the table, which
        // stays behind a raw pointer; `finish_init` writes the other fields
        // and reads only those rows. Later rows stay uninitialized until
        // `generate_rows` writes them.
        unsafe {
            let table = core::ptr::addr_of_mut!((*p).table).cast::<u8>();
            aes.ctr2(label, table, EAGER_ROWS * BLOCK / 16, (TABLE_BYTES / 16) as u64, fields.as_mut_ptr(), fields.len() / 16);
            Self::finish_init(p, &fields, backend);
            core::ptr::addr_of_mut!((*p).lazy).write(Lazy {
                ready: AtomicU8::new(EAGER_ROWS as u8),
                lock: AtomicBool::new(false),
                source: Some((seed, label)),
            });
        }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// `hash`, then the avalanche finalizer: `avalanche_mix(H, tweak) * V`
    /// for a fixed bijection and an independent key element `V`. Outputs
    /// look unrelated across tweaks, and the result stays
    /// almost-XOR-universal with bound `epsilon + 2^-128`. Unless `V = 0`
    /// (a `2^-128` fraction of keys, which makes every output 0),
    /// collisions are exactly those of `hash`, for every tweak. Like `hash`,
    /// never expose it to an adversary who chooses messages; use `Mac` for
    /// that.
    pub fn hash_avalanche(&self, msg: &[u8], tweak: u64) -> u128 {
        self.avalanche(self.hash(msg), tweak)
    }

    fn avalanche(&self, raw: u128, tweak: u64) -> u128 {
        let m = reference::avalanche_mix(raw, tweak);
        #[cfg(target_arch = "x86_64")]
        if self.backend != Backend::Portable {
            // SAFETY: every x86 backend has PCLMULQDQ.
            return unsafe { x86::mul_x64(m, &self.vk) };
        }
        self.mul(m, &self.v)
    }

    /// `x k` in the field, with the backend's carryless multiply.
    fn mul(&self, x: u128, k: &FieldKey) -> u128 {
        match self.backend {
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            // SAFETY: the backend is supported.
            Backend::NeonEor3 | Backend::NeonPlain => unsafe { neon::mul_key(x, k) },
            #[cfg(target_arch = "x86_64")]
            // SAFETY: every x86 backend has PCLMULQDQ.
            b if b != Backend::Portable => unsafe { x86::mul_key(x, k) },
            _ => portable::gf_mul(x, k.k),
        }
    }

    /// `a^e` in the field, with `mul`.
    #[cfg(feature = "std")]
    fn pow(&self, a: u128, mut e: u64) -> u128 {
        let (mut base, mut acc) = (a, 1u128);
        while e > 0 {
            if e & 1 == 1 {
                acc = self.mul(acc, &FieldKey::new(base));
            }
            base = self.mul(base, &FieldKey::new(base));
            e >>= 1;
        }
        acc
    }

    /// Hash `msg` in one call.
    #[inline]
    pub fn hash(&self, msg: &[u8]) -> u128 {
        let len = msg.len();
        if len < SHORT_MAX {
            return self.short::<false>(msg);
        }
        if len <= S2_MAX {
            return self.medium(msg);
        }
        if len <= GROUP {
            #[cfg(target_arch = "x86_64")]
            if self.backend == Backend::X86Avx512 {
                // Its own kernel rather than the generic `finish` (every
                // backend, any `Core`): 7-17% faster on Zen 5.
                // SAFETY: the backend is supported, a fresh group's rows are
                // among the `EAGER_ROWS`, and `S2_MAX < len <= GROUP`.
                return unsafe { x86::avx512::group(self, msg) };
            }
            // Up to one group goes straight to the final-chunk routine.
            return self.finish(&FRESH, msg, len as u64);
        }
        self.hash_long(msg)
    }

    /// Hash `msg` using up to `threads` threads (the calling one included,
    /// and fewer for inputs too small to repay starting them); identical
    /// output to `hash`.
    ///
    /// Whole chunks are split into tasks of 256 KiB taken dynamically by
    /// the threads (so slower cores simply take fewer). Because each outer
    /// step is `P <- P R + c`, a task started from zero yields `c_task`, and
    /// the tasks combine in order as `P <- P R^m + c_task`. Worth it only for
    /// large inputs; thread start-up costs tens of microseconds.
    #[cfg(feature = "std")]
    pub fn hash_parallel(&self, msg: &[u8], threads: usize) -> u128 {
        use std::sync::atomic::{AtomicUsize, Ordering};

        const TASK_CHUNKS: usize = 32;
        let whole = msg.len() / CHUNK;
        if threads <= 1 || whole < 2 * TASK_CHUNKS {
            return self.hash(msg);
        }
        let tasks = whole.div_ceil(TASK_CHUNKS);
        // Starting a thread costs about as much as hashing a few hundred KiB
        // on one core, so use one thread per `PAR_MIN_BYTES` at most.
        let threads = threads.min(tasks).min(msg.len() / PAR_MIN_BYTES);
        if threads <= 1 {
            return self.hash(msg);
        }
        self.ensure_rows(TABLE_ROWS);
        let results: std::vec::Vec<std::sync::Mutex<u128>> = (0..tasks).map(|_| std::sync::Mutex::new(0)).collect();
        let next = AtomicUsize::new(0);
        let work = || loop {
            let t = next.fetch_add(1, Ordering::Relaxed);
            if t >= tasks {
                break;
            }
            let (a, b) = (t * TASK_CHUNKS, ((t + 1) * TASK_CHUNKS).min(whole));
            let mut core = Core::new();
            self.absorb(&mut core, &msg[a * CHUNK..b * CHUNK]);
            *results[t].lock().unwrap() = core.outer;
        };
        std::thread::scope(|scope| {
            // The calling thread works too.
            for _ in 1..threads {
                scope.spawn(work);
            }
            work();
        });
        // Combine in message order: P <- P * R^m + c_task.
        let r_full = FieldKey::new(self.pow(self.r.k, TASK_CHUNKS as u64));
        let mut outer = 0u128;
        for (t, c) in results.iter().enumerate() {
            let m = ((t + 1) * TASK_CHUNKS).min(whole) - t * TASK_CHUNKS;
            let rm = if m == TASK_CHUNKS { r_full } else { FieldKey::new(self.pow(self.r.k, m as u64)) };
            outer = self.mul(outer, &rm) ^ *c.lock().unwrap();
        }
        let mut core = Core::new();
        core.outer = outer;
        core.closed = true;
        let rest = &msg[whole * CHUNK..];
        let groups = rest.len() / GROUP * GROUP;
        self.absorb(&mut core, &rest[..groups]);
        self.finish(&core, &rest[groups..], msg.len() as u64)
    }

    /// Kept out of line so callers of `hash` do not carry its state frame.
    #[inline(never)]
    fn hash_long(&self, msg: &[u8]) -> u128 {
        #[cfg(target_arch = "x86_64")]
        if self.backend == Backend::X86Avx512 {
            // Whole chunks, then the final partial chunk in one kernel.
            let whole = msg.len() / CHUNK * CHUNK;
            if whole == 0 {
                // Rows up to the end position of the one partial chunk.
                self.ensure_rows(msg.len().div_ceil(BLOCK) + 1);
                // SAFETY: the backend is supported; those rows are final and
                // `msg` is a partial chunk.
                return unsafe { x86::avx512::tail(self, msg, None, msg.len() as u64) };
            }
            let mut core = MaybeUninit::<Core>::uninit();
            // SAFETY: `init_at` writes every field that is not `MaybeUninit`.
            let mut core = unsafe {
                Core::init_at(core.as_mut_ptr());
                core.assume_init()
            };
            self.absorb(&mut core, &msg[..whole]);
            let rest = &msg[whole..];
            if rest.is_empty() {
                return self.finish(&core, rest, msg.len() as u64);
            }
            let outer = core.closed.then_some(core.outer);
            // SAFETY: the backend is supported; `absorb` made the table
            // complete and `rest` is a partial chunk.
            return unsafe { x86::avx512::tail(self, rest, outer, msg.len() as u64) };
        }
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        if matches!(self.backend, Backend::NeonEor3 | Backend::NeonPlain) {
            // Whole chunks, then the final partial chunk in one kernel.
            let whole = msg.len() / CHUNK * CHUNK;
            let mut core = MaybeUninit::<Core>::uninit();
            // SAFETY: `init_at` writes every field that is not `MaybeUninit`.
            let mut core = unsafe {
                Core::init_at(core.as_mut_ptr());
                core.assume_init()
            };
            self.absorb(&mut core, &msg[..whole]);
            let rest = &msg[whole..];
            if rest.is_empty() {
                return self.finish(&core, rest, msg.len() as u64);
            }
            let outer = core.closed.then_some(core.outer);
            // SAFETY: the backend is supported; `absorb` made the table
            // complete and `rest` is a partial chunk.
            return unsafe {
                match self.backend {
                    Backend::NeonEor3 => neon::eor3::tail(self, rest, outer, msg.len() as u64),
                    _ => neon::plain::tail(self, rest, outer, msg.len() as u64),
                }
            };
        }
        let mut core = MaybeUninit::<Core>::uninit();
        // SAFETY: `init_at` writes every field that is not `MaybeUninit`.
        let mut core = unsafe {
            Core::init_at(core.as_mut_ptr());
            core.assume_init()
        };
        let whole = msg.len() / GROUP * GROUP;
        self.absorb(&mut core, &msg[..whole]);
        self.finish(&core, &msg[whole..], msg.len() as u64)
    }

    /// Start an incremental hash.
    #[inline]
    pub fn hasher(&self) -> Hasher<'_> {
        let mut h = MaybeUninit::<Hasher<'_>>::uninit();
        let p = h.as_mut_ptr();
        // SAFETY: every field that is not `MaybeUninit` is written.
        unsafe {
            core::ptr::addr_of_mut!((*p).key).write(self);
            Core::init_at(core::ptr::addr_of_mut!((*p).core));
            core::ptr::addr_of_mut!((*p).buf_len).write(0);
            core::ptr::addr_of_mut!((*p).len).write(0);
            h.assume_init()
        }
    }

    /// A message of fewer than `SHORT_MAX` bytes. `STORED`: its bytes were
    /// just written by `Hasher::update`, which AVX2's dword-masked loads do
    /// not forward from (a stall of about 7 ns), so that backend uses the
    /// narrow loads below instead.
    fn short<const STORED: bool>(&self, msg: &[u8]) -> u128 {
        let len = msg.len();
        if len == 0 {
            // `0 * A + S`.
            return self.s;
        }
        #[cfg(target_arch = "x86_64")]
        match self.backend {
            // SAFETY (both): the backend is supported and `0 < len < SHORT_MAX`.
            Backend::X86Avx512 => return unsafe { x86::avx512::short_msg(self, msg) },
            Backend::X86Avx2 if !STORED => return unsafe { x86::short_avx2(self, msg) },
            // SAFETY: the backend is supported and the CPU has AVX2.
            Backend::X86Sse if !STORED && self.avx2 => return unsafe { x86::short_avx2_x(self, msg) },
            _ => {},
        }
        self.short_narrow(msg)
    }

    /// `short` through narrow loads, for every backend. Out of line, so
    /// that the vector paths above inline without its register saves.
    #[inline(never)]
    fn short_narrow(&self, msg: &[u8]) -> u128 {
        let len = msg.len();
        // The length byte sits in byte 15 of the last element.
        let marker = (len as u128) << 120;
        if len < 16 {
            return self.dispatch_short(partial16(msg, 0) | marker, None);
        }
        let x0 = partial16(&msg[..16], 0);
        // The tail through its own slice: narrow loads within bytes 16..len
        // (see `Hasher::update`).
        self.dispatch_short(x0, Some(partial16(msg, 16) | marker))
    }
}

impl Key {
    /// Absorb whole groups, closing chunks as they fill.
    fn absorb(&self, core: &mut Core, mut data: &[u8]) {
        debug_assert_eq!(data.len() % GROUP, 0);
        self.ensure_rows(TABLE_ROWS);
        #[cfg(target_arch = "x86_64")]
        if self.backend == Backend::X86Avx512 {
            // SAFETY: the backend is supported and the table is complete.
            return unsafe { x86::avx512::absorb(core, self, data) };
        }
        while !data.is_empty() {
            if core.pos == 0 && data.len() >= CHUNK {
                let n = data.len() / CHUNK;
                let (now, later) = data.split_at(n * CHUNK);
                dispatch!(
                    self,
                    for c in now.as_chunks::<GROUP>().0 {
                        portable::groups(core, self, c);
                    },
                    |m| m::chunks(core, self, now.as_ptr(), n)
                );
                data = later;
                continue;
            }
            let n = (data.len() / GROUP).min((params::CHUNK_BLOCKS - core.pos) / GROUP_BLOCKS);
            let (now, later) = data.split_at(n * GROUP);
            dispatch!(self, portable::groups(core, self, now), |m| m::groups(core, self, now.as_ptr(), n));
            data = later;
        }
    }

    /// The output after `core` and at most `GROUP` pending bytes.
    fn finish(&self, core: &Core, pending: &[u8], len: u64) -> u128 {
        // Rows up to the end position, one past the last pending block: at
        // a fresh chunk these are among the `EAGER_ROWS`; after a closed chunk
        // or within one, `absorb` made the table complete.
        debug_assert!(core.pos + pending.len().div_ceil(BLOCK) < EAGER_ROWS || core.pos > 0 || core.closed);
        dispatch!(self, portable::finish(core, self, pending, len), |m| m::finish(core, self, pending, len))
    }

    fn dispatch_short(&self, x0: u128, x1: Option<u128>) -> u128 {
        dispatch!(self, portable::short(self, x0, x1), |m| m::short(self, x0, x1))
    }

    /// A message of `SHORT_MAX..=S2_MAX` bytes.
    fn medium(&self, msg: &[u8]) -> u128 {
        #[cfg(target_arch = "x86_64")]
        if self.backend == Backend::X86Avx2 {
            // SAFETY: the backend is supported; `SHORT_MAX <= len <= S2_MAX`.
            return unsafe { x86::medium_avx2(self, msg) };
        }
        self.medium_stored(msg)
    }

    /// `medium` for bytes just written by `Hasher::update`: AVX2 keeps the
    /// 128-bit form, whose 16-byte loads forward from those stores.
    fn medium_stored(&self, msg: &[u8]) -> u128 {
        dispatch!(self, portable::finish(&FRESH, self, msg, msg.len() as u64), |m| m::medium(self, msg))
    }
}

/// A group of pending bytes, aligned to a cache line so that the kernels'
/// 64-byte loads from it never straddle two lines.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct GroupBuf([u8; GROUP]);

/// Incremental hashing state (about 1.5 KB), borrowing its key.
///
/// Feeding a message in any number of `update` calls gives the same result
/// as `Key::hash` on the concatenation. Whole 1 KiB groups are absorbed
/// straight from the input once more data follows them; up to 1 KiB is
/// buffered, so a message of at most one group is hashed in one pass by
/// `finalize`.
#[derive(Clone)]
pub struct Hasher<'a> {
    key: &'a Key,
    core: Core,
    /// Pending bytes not yet absorbed; only `..buf_len` is initialized.
    buf: MaybeUninit<GroupBuf>,
    buf_len: usize,
    len: u64,
}

impl Hasher<'_> {
    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        let total = self.buf_len + data.len();
        if total < GROUP {
            // Common small case: just buffer.
            self.len = self.len.checked_add(data.len() as u64).expect("message longer than 2^64 - 1 bytes");
            let dst = self.buf.as_mut_ptr() as *mut u8;
            #[cfg(target_arch = "x86_64")]
            if self.buf_len == 0 && self.key.backend == Backend::X86Avx512 {
                // SAFETY: the backend is supported; `data.len() <= GROUP`, so
                // the rounded-up copy stays within the buffer.
                unsafe { x86::avx512::copy_group(dst, data) };
                self.buf_len = total;
                return;
            }
            // SAFETY: `buf_len + data.len() <= GROUP`.
            unsafe { copy_small(dst.add(self.buf_len), data) };
            self.buf_len = total;
            return;
        }
        self.update_long(data);
    }

    /// `buf_len + data.len() >= GROUP`: whole groups are absorbed, except a
    /// message's first group when the data ends with it, which stays
    /// buffered so that `finalize` can hash a message of one group in one
    /// pass. (Buffering every group-completing update instead cost 11% for
    /// 1 KiB updates; deciding it in `update` cost 10-20% for small ones.)
    #[inline(never)]
    fn update_long(&mut self, mut data: &[u8]) {
        let first = self.len == self.buf_len as u64;
        self.len = self.len.checked_add(data.len() as u64).expect("message longer than 2^64 - 1 bytes");
        let buf = self.buf.as_mut_ptr() as *mut u8;
        if first && self.buf_len + data.len() == GROUP {
            #[cfg(target_arch = "x86_64")]
            if self.buf_len == 0 && self.key.backend == Backend::X86Avx512 {
                // SAFETY: the backend is supported; the copy is one group.
                unsafe { x86::avx512::copy_group(buf, data) };
                self.buf_len = GROUP;
                return;
            }
            // SAFETY: `buf_len + data.len() == GROUP`.
            unsafe { copy_small(buf.add(self.buf_len), data) };
            self.buf_len = GROUP;
            return;
        }
        if self.buf_len > 0 {
            let take = GROUP - self.buf_len;
            // SAFETY: `buf_len + take == GROUP`.
            unsafe { copy_small(buf.add(self.buf_len), &data[..take]) };
            data = &data[take..];
            // SAFETY: all GROUP bytes are initialized.
            let group = unsafe { &self.buf.assume_init_ref().0 };
            self.key.absorb(&mut self.core, group);
            self.buf_len = 0;
        }
        let whole = data.len() / GROUP * GROUP;
        self.key.absorb(&mut self.core, &data[..whole]);
        let tail = &data[whole..];
        self.buf_len = tail.len();
        #[cfg(target_arch = "x86_64")]
        if self.key.backend == Backend::X86Avx512 {
            // SAFETY: the backend is supported; `tail.len() < GROUP`, so the
            // rounded-up copy stays within the buffer.
            unsafe { x86::avx512::copy_group(buf, tail) };
            return;
        }
        // SAFETY: `tail.len() < GROUP`.
        unsafe { copy_small(buf, tail) };
    }

    fn pending(&self) -> &[u8] {
        // SAFETY: the first `buf_len` bytes were written by `update`.
        unsafe { core::slice::from_raw_parts(self.buf.as_ptr() as *const u8, self.buf_len) }
    }

    /// Number of bytes absorbed so far.
    pub fn count(&self) -> u64 {
        self.len
    }

    /// The hash of everything absorbed so far; the hasher stays usable.
    #[inline]
    pub fn finalize(&self) -> u128 {
        if self.len < SHORT_MAX as u64 {
            return self.key.short::<true>(self.pending());
        }
        if self.len <= S2_MAX as u64 {
            return self.key.medium_stored(self.pending());
        }
        #[cfg(target_arch = "x86_64")]
        if self.len <= GROUP as u64 && self.key.backend == Backend::X86Avx512 {
            // All of the message is pending (as in `Key::hash`).
            // SAFETY: the backend is supported, a fresh group's rows are
            // among the `EAGER_ROWS`, and `S2_MAX < len <= GROUP`.
            return unsafe { x86::avx512::group(self.key, self.pending()) };
        }
        self.key.finish(&self.core, self.pending(), self.len)
    }

    /// `finalize` with the avalanche finalizer of `Key::hash_avalanche`.
    pub fn finalize_avalanche(&self, tweak: u64) -> u128 {
        self.key.avalanche(self.finalize(), tweak)
    }

    pub fn reset(&mut self) {
        // SAFETY: `self.core` is valid for writes.
        unsafe { Core::init_at(&mut self.core) };
        self.buf_len = 0;
        self.len = 0;
    }
}

#[cfg(feature = "aes")]
const DOMAIN_KEY: u64 = 0x6b65_795f_6331_3238; // "c128_yek" little-endian label

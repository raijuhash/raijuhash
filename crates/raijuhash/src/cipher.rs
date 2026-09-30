//! AES-128 for seed expansion and MAC masking: the ARMv8 AES instructions or
//! AES-NI (with VAES) when the CPU has them (same outputs, without the `aes`
//! crate's per-call dispatch), else the `aes` crate.

use aes::Aes128;
use aes::cipher::{BlockCipherEncrypt, KeyInit};

// The software variant is larger; boxing it would need `alloc`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub(crate) enum Aes {
    /// Round keys for `crate::aes_neon`.
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    Neon([u128; 11]),
    /// Round keys for `crate::x86::aes`, and whether counter mode may use
    /// 512-bit VAES.
    #[cfg(target_arch = "x86_64")]
    Ni([u128; 11], bool),
    Soft(Aes128),
}

impl Aes {
    pub fn new(key: u128) -> Aes {
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        if crate::Backend::NeonPlain.supported() {
            // SAFETY: the CPU supports `neon` and `aes`.
            return Aes::Neon(unsafe { crate::aes_neon::schedule(key) });
        }
        #[cfg(target_arch = "x86_64")]
        if crate::x86::aes::supported() {
            // SAFETY: the CPU supports `aes` and `sse4.1`.
            return Aes::Ni(unsafe { crate::x86::aes::schedule(key) }, crate::x86::aes::wide_supported());
        }
        Aes::Soft(Aes128::new(&key.to_le_bytes().into()))
    }

    /// Write the `n` encrypted counter blocks `label << 64 | (first + i)`
    /// (little-endian) to `out`.
    ///
    /// # Safety
    /// `out` must be valid for writes of `16 n` bytes. They may be
    /// uninitialized: every block is written before any is borrowed.
    pub unsafe fn ctr(&self, label: u64, first: u64, out: *mut u8, n: usize) {
        match self {
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            // SAFETY: `Neon` is only built when the CPU supports it.
            Aes::Neon(rk) => unsafe { crate::aes_neon::ctr(rk, label, first, out, n) },
            #[cfg(target_arch = "x86_64")]
            // SAFETY: `Ni` is only built when the CPU supports it, with
            // `wide` only when it also has VAES and AVX-512F.
            Aes::Ni(rk, wide) => unsafe {
                if *wide {
                    crate::x86::aes::ctr_wide(rk, label, first, out, n)
                } else {
                    crate::x86::aes::ctr(rk, label, first, out, n)
                }
            },
            Aes::Soft(c) => {
                // `aes::Block` is a transparent wrapper around `[u8; 16]`.
                let blocks = out.cast::<aes::Block>();
                for i in 0..n {
                    let ctr = (label as u128) << 64 | (first + i as u64) as u128;
                    // SAFETY: block `i` lies within `out`.
                    unsafe { blocks.add(i).write(ctr.to_le_bytes().into()) };
                }
                // SAFETY: all `n` blocks were initialized above.
                c.encrypt_blocks(unsafe { core::slice::from_raw_parts_mut(blocks, n) });
            },
        }
    }

    /// `ctr` of `first_n` blocks from counter 0 to `first` and of `rest_n`
    /// blocks from counter `rest_first` to `rest`: a seeded key's eager
    /// rows and field elements, in one batch where the CPU allows.
    ///
    /// # Safety
    /// As for `ctr`, for both outputs.
    pub unsafe fn ctr2(&self, label: u64, first: *mut u8, first_n: usize, rest_first: u64, rest: *mut u8, rest_n: usize) {
        #[cfg(target_arch = "x86_64")]
        if let Aes::Ni(rk, true) = self
            && first_n == 72
            && rest_n == 7
        {
            // SAFETY: `wide` is only set when the CPU has VAES and AVX-512F;
            // the output sizes match.
            return unsafe { crate::x86::aes::eager_wide::<18>(rk, label, first, rest_first, rest) };
        }
        // SAFETY: as for this function.
        unsafe {
            self.ctr(label, 0, first, first_n);
            self.ctr(label, rest_first, rest, rest_n);
        }
    }

    /// `E(a) XOR E(b)`, blocks as little-endian integers.
    #[inline]
    pub fn encrypt2_xor(&self, a: u128, b: u128) -> u128 {
        match self {
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            // SAFETY: `Neon` is only built when the CPU supports it.
            Aes::Neon(rk) => unsafe { crate::aes_neon::encrypt2_xor(rk, a, b) },
            #[cfg(target_arch = "x86_64")]
            // SAFETY: `Ni` is only built when the CPU supports it.
            Aes::Ni(rk, _) => unsafe { crate::x86::aes::encrypt2_xor(rk, a, b) },
            Aes::Soft(c) => {
                let mut blocks = [a.to_le_bytes().into(), b.to_le_bytes().into()];
                c.encrypt_blocks(&mut blocks);
                u128::from_le_bytes(blocks[0].into()) ^ u128::from_le_bytes(blocks[1].into())
            },
        }
    }
}

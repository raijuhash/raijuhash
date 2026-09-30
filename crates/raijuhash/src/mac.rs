//! Nonce-based message authentication built on the universal hash.
//!
//! Tags follow the nonce-based enhanced hash-then-mask shape (nEHtM,
//! Dutta, Nandi and Talnikar): for a 127-bit nonce `N` and the hash `H(M)`
//! truncated to 127 bits,
//!
//! ```text
//! T = AES_K(0 || N)  XOR  AES_K(1 || (N XOR H(M)))
//! ```
//!
//! where the leading bit is the most significant bit of the block. A `Mac`
//! has its own hash key, so raw hashes of that key are never exposed.
//!
//! # Usage limits
//!
//! Per key, with messages of at most `L` bytes, `q` tags, `v` verifications
//! and `delta = (ceil(L / 8192) + 1) / 2^127`, the forging probability is at
//! most `(2v + 29q + 12) * delta` when nonces never repeat and `q <= 2^64`,
//! plus the PRP advantage of AES-128. Section 6 of `SPEC.md` derives this
//! from the published nEHtM bounds and tabulates exact values, including
//! repeated nonces. Recommended limits per key:
//!
//! - messages of at most 2^32 bytes;
//! - at most 2^48 tags and 2^48 failed verifications (bound 2^-60);
//! - never repeat a nonce. Nonces equal in their low 127 bits are the same
//!   nonce. Occasional repeats degrade security gracefully rather than
//!   revealing the key, but each one counts against the bound.

use subtle::ConstantTimeEq;

use crate::cipher::Aes;
use crate::{Hasher, KEY_BYTES, Key};

const DOMAIN_MAC_HASH: u64 = 0x6873_685f_6361_6d63; // label for the hash key
const DOMAIN_MAC_CIPHER: u64 = 0x7068_635f_6361_6d63; // label for the AES key
const TOP: u128 = 1 << 127;

/// A MAC key: a universal-hash key plus an AES-128 key.
#[derive(Clone)]
pub struct Mac {
    key: Key,
    cipher: Aes,
}

impl core::fmt::Debug for Mac {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Mac").finish_non_exhaustive()
    }
}

impl Mac {
    /// Derive both keys from a 128-bit secret seed, in domains disjoint from
    /// `Key::from_seed`.
    pub fn from_seed(seed: u128) -> Mac {
        let aes = Aes::new(seed);
        let mut k = [0u8; 16];
        // SAFETY: `k` is one block.
        unsafe { aes.ctr(DOMAIN_MAC_CIPHER, 0, k.as_mut_ptr(), 1) };
        Mac { key: Key::expand(seed, &aes, DOMAIN_MAC_HASH), cipher: Aes::new(u128::from_le_bytes(k)) }
    }

    /// Build from explicit uniformly random hash-key bytes and AES key.
    pub fn from_parts(hash_key: &[u8; KEY_BYTES], aes_key: u128) -> Mac {
        Mac { key: Key::from_entropy(hash_key), cipher: Aes::new(aes_key) }
    }

    /// Tag `msg` under `nonce`; only the low 127 bits of `nonce` are used.
    pub fn tag(&self, nonce: u128, msg: &[u8]) -> u128 {
        self.mask(nonce, self.key.hash(msg))
    }

    /// Check a tag; see `tags_equal` for the timing properties.
    pub fn verify(&self, nonce: u128, msg: &[u8], tag: u128) -> bool {
        tags_equal(self.tag(nonce, msg), tag)
    }

    /// Incremental tagging.
    pub fn hasher(&self) -> MacHasher<'_> {
        MacHasher { inner: self.key.hasher(), mac: self }
    }

    fn mask(&self, nonce: u128, h: u128) -> u128 {
        let n = nonce & !TOP;
        self.cipher.encrypt2_xor(n, TOP | (n ^ h))
    }
}

/// Incremental state for `Mac`.
#[derive(Clone)]
pub struct MacHasher<'a> {
    inner: Hasher<'a>,
    mac: &'a Mac,
}

impl MacHasher<'_> {
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize(&self, nonce: u128) -> u128 {
        self.mac.mask(nonce, self.inner.finalize())
    }

    pub fn verify(&self, nonce: u128, tag: u128) -> bool {
        tags_equal(self.finalize(nonce), tag)
    }
}

/// Whether two tags are equal, revealing nothing else about them.
///
/// `subtle` compares all 128 bits with arithmetic and passes the one-bit
/// result through an optimization barrier (a volatile read in a function
/// that is never inlined), so the compiler cannot turn the comparison into
/// an early exit on the first differing part. Only the final accept/reject
/// becomes a branch, and that outcome is public anyway. This is a property
/// of the generated code, not a language guarantee; it was checked in the
/// AArch64 release build (see `SPEC.md` section 6).
fn tags_equal(a: u128, b: u128) -> bool {
    a.ct_eq(&b).into()
}

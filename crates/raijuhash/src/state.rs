//! Backend-independent state of the chunk currently being absorbed.

use core::mem::MaybeUninit;

use crate::params::BLOCK;

/// Blocks per group; state only ever advances by whole groups.
pub const GROUP_BLOCKS: usize = 8;
/// Bytes per group.
pub const GROUP: usize = GROUP_BLOCKS * BLOCK;

/// Chunk in progress, in the layout of the backend's group kernels. For the
/// SIMD backends words 0..8 hold `B[v]` (product sums of positions `8u + v`),
/// words 8..11 the planes `F[b]` (sums over positions whose `u` has bit `b`)
/// and words 16..24 the previous block; the portable backend keeps `C[u]`
/// in words 8..16 instead. `pos` counts absorbed blocks and is always a
/// multiple of eight. At `pos == 0` the words are logically zero and may be
/// uninitialized, which keeps creating a hasher cheap. The AVX-512 backend
/// keeps only the sums `h0` and `h1` (words 0 and 1) and the previous block,
/// and reads them through `words_raw` rather than `words`, since the other
/// words stay uninitialized.
#[derive(Clone)]
#[repr(C, align(16))]
pub struct Core {
    words: MaybeUninit<[u128; 24]>,
    pub pos: usize,
    pub outer: u128,
    /// Whether any chunk was closed, i.e. whether `outer` is in use.
    pub closed: bool,
}

impl Core {
    pub const fn new() -> Self {
        Core { words: MaybeUninit::uninit(), pos: 0, outer: 0, closed: false }
    }

    /// Initialize a `Core` in place, writing only the live fields so the
    /// state words are left untouched (a by-value `Core::new()` can end up
    /// zero-filling them).
    ///
    /// # Safety
    /// `p` must be valid for writes.
    #[inline(always)]
    pub unsafe fn init_at(p: *mut Core) {
        unsafe {
            core::ptr::addr_of_mut!((*p).pos).write(0);
            core::ptr::addr_of_mut!((*p).outer).write(0);
            core::ptr::addr_of_mut!((*p).closed).write(false);
        }
    }

    /// The state words, or `None` at a chunk start.
    pub fn words(&self) -> Option<&[u128; 24]> {
        // SAFETY: `pos > 0` only after `words_mut` initialized the words.
        (self.pos > 0).then(|| unsafe { self.words.assume_init_ref() })
    }

    /// The state words, for a backend that reads only the words it wrote.
    #[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
    pub fn words_raw(&self) -> *const u128 {
        self.words.as_ptr() as *const u128
    }

    /// Raw storage for a backend that initializes the words it uses before
    /// making `pos` nonzero.
    #[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
    pub fn words_ptr(&mut self) -> *mut u128 {
        self.words.as_mut_ptr() as *mut u128
    }

    /// Mutable state words, zeroed first at a chunk start.
    pub fn words_mut(&mut self) -> &mut [u128; 24] {
        if self.pos == 0 {
            self.words.write([0; 24]);
        }
        // SAFETY: initialized above or by an earlier call.
        unsafe { self.words.assume_init_mut() }
    }
}

/// A block holding `src` (fewer than 128 bytes) followed by zeros, built
/// without a `memcpy` call: whole 16-byte pieces use fixed-size moves (a
/// constant trip count, so no loop is turned back into `memcpy`), and the
/// final partial piece is assembled in a register and stored once. Every
/// store is aligned and non-overlapping, so later 16-byte loads forward.
#[inline(always)]
pub fn padded_block(src: &[u8]) -> [u8; BLOCK] {
    let n = src.len();
    assert!(n < BLOCK);
    let mut dst = [0u8; BLOCK];
    let whole = n / 16;
    for i in 0..BLOCK / 16 {
        if i < whole {
            // SAFETY: piece `i` lies within `src` and `dst`.
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr().add(16 * i), dst.as_mut_ptr().add(16 * i), 16) };
        }
    }
    if n % 16 != 0 {
        let v = partial16(src, 16 * whole);
        // SAFETY: `16 * whole + 16 <= BLOCK` since `n < BLOCK`.
        unsafe { core::ptr::write_unaligned(dst.as_mut_ptr().add(16 * whole) as *mut u128, v.to_le()) };
    }
    dst
}

/// Bytes `src[at..]` (at most 16) as a little-endian integer, zero-padded.
/// Reads only bytes of `src`.
#[inline(always)]
pub fn partial16(src: &[u8], at: usize) -> u128 {
    let n = src.len();
    assert!(at <= n && n - at <= 16);
    let r = n - at;
    let p = src.as_ptr();
    // SAFETY (all reads below): every address read lies in `src[..n]`.
    unsafe {
        if r == 16 {
            return u128::from_le(core::ptr::read_unaligned(p.add(at) as *const u128));
        }
        if n >= 16 {
            // The last 16 bytes of `src`, shifted to drop bytes before `at`.
            let v = u128::from_le(core::ptr::read_unaligned(p.add(n - 16) as *const u128));
            return if r == 0 { 0 } else { v >> (8 * (16 - r)) };
        }
        let q = p.add(at);
        let rd4 = |o: usize| u32::from_le(core::ptr::read_unaligned(q.add(o) as *const u32)) as u128;
        let rd8 = |o: usize| u64::from_le(core::ptr::read_unaligned(q.add(o) as *const u64)) as u128;
        // Two overlapping reads of one width cover r bytes exactly.
        if r >= 8 {
            rd8(0) | rd8(r - 8) << (8 * (r - 8))
        } else if r >= 4 {
            rd4(0) | rd4(r - 4) << (8 * (r - 4))
        } else if r > 0 {
            *q as u128 | (*q.add(r / 2) as u128) << (8 * (r / 2)) | (*q.add(r - 1) as u128) << (8 * (r - 1))
        } else {
            0
        }
    }
}

/// Copy a slice to `dst`: short copies use fixed-size, possibly overlapping
/// moves (a `memcpy` call costs more than the copy), longer ones `memcpy`.
///
/// # Safety
/// `dst` must be writable for `src.len()` bytes and `src.len() <= 1024`.
#[inline(always)]
pub unsafe fn copy_small(dst: *mut u8, src: &[u8]) {
    let n = src.len();
    let s = src.as_ptr();
    // SAFETY: every move stays within `src[..n]` and `dst[..n]`.
    unsafe {
        let mv = |o: usize, w: usize| core::ptr::copy_nonoverlapping(s.add(o), dst.add(o), w);
        if n >= 64 {
            // Long enough for the platform `memcpy` to pay off.
            core::ptr::copy_nonoverlapping(s, dst, n);
        } else if n >= 16 {
            mv(0, 16);
            if n > 32 {
                mv(16, 16);
            }
            if n > 48 {
                mv(32, 16);
            }
            mv(n - 16, 16);
        } else if n >= 8 {
            mv(0, 8);
            mv(n - 8, 8);
        } else if n >= 4 {
            mv(0, 4);
            mv(n - 4, 4);
        } else if n > 0 {
            mv(0, 1);
            mv(n / 2, 1);
            mv(n - 1, 1);
        }
    }
}

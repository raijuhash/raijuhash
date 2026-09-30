//! Key parameters and their byte layout.

/// Bytes per block: an X row of eight 64-bit words and a Y row of eight.
pub const BLOCK: usize = 128;
/// Blocks per chunk.
pub const CHUNK_BLOCKS: usize = 64;
/// Bytes per chunk.
pub const CHUNK: usize = BLOCK * CHUNK_BLOCKS;
/// Rows in the chain key table: one per encoded position 0..=64.
pub const TABLE_ROWS: usize = CHUNK_BLOCKS + 1;
/// Bytes of chain key table.
pub const TABLE_BYTES: usize = TABLE_ROWS * BLOCK;
/// Messages shorter than this use the short path.
pub const SHORT_MAX: usize = 32;
/// Bytes of uniformly random parameters a key consists of.
pub const KEY_BYTES: usize = TABLE_BYTES + 7 * 16;

/// The mathematical key: every field must be independent and uniform for the
/// stated bound to hold. Field elements are little-endian `u128`s with bit
/// `i` the coefficient of `x^i`.
#[derive(Clone)]
pub struct Params {
    /// Chain key rows; words 0..8 key the X row, 8..16 the Y row.
    pub table: [[u64; 16]; TABLE_ROWS],
    /// Short-path coefficients.
    pub a: u128,
    pub b: u128,
    /// Outer polynomial evaluation points.
    pub r: u128,
    pub r2: u128,
    /// Length coefficient.
    pub t: u128,
    /// Output offset.
    pub s: u128,
    /// Avalanche finalizer multiplier.
    pub v: u128,
}

impl Params {
    /// Parse `KEY_BYTES` bytes: the table row by row, then a, b, r, r2, t, s, v.
    pub fn from_bytes(bytes: &[u8; KEY_BYTES]) -> Self {
        let mut table = [[0u64; 16]; TABLE_ROWS];
        for (row, src) in table.iter_mut().zip(bytes[..TABLE_BYTES].chunks_exact(BLOCK)) {
            for (w, b) in row.iter_mut().zip(src.chunks_exact(8)) {
                *w = u64::from_le_bytes(b.try_into().unwrap());
            }
        }
        let f = |i: usize| {
            let o = TABLE_BYTES + 16 * i;
            u128::from_le_bytes(bytes[o..o + 16].try_into().unwrap())
        };
        Params { table, a: f(0), b: f(1), r: f(2), r2: f(3), t: f(4), s: f(5), v: f(6) }
    }
}

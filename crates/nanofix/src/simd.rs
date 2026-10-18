/// SIMD-accelerated SOH (0x01) delimiter scanning for FIX message parsing.
///
/// Provides vectorized byte scanning using NEON (aarch64) or SSE2 (x86_64)
/// intrinsics with automatic runtime feature detection and scalar fallback.

/// Find the index of the first SOH (0x01) byte in `buf`.
/// Returns `None` if no SOH is found.
#[inline]
pub fn find_soh(buf: &[u8]) -> Option<usize> {
    find_byte(buf, crate::tags::SOH)
}

/// Count the number of SOH (0x01) delimiters in `buf`.
/// Useful for pre-sizing field arrays before parsing.
#[inline]
pub fn count_fields(buf: &[u8]) -> usize {
    count_byte(buf, crate::tags::SOH)
}

/// Find the index of the first `=` byte in `buf`.
/// Used to split tag=value pairs.
#[inline]
pub fn find_equals(buf: &[u8]) -> Option<usize> {
    find_byte(buf, crate::tags::EQUALS)
}

// ---------------------------------------------------------------------------
// aarch64 NEON implementation
// ---------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
#[inline]
fn find_byte(buf: &[u8], needle: u8) -> Option<usize> {
    if std::arch::is_aarch64_feature_detected!("neon") {
        // SAFETY: we have verified NEON support via runtime detection and all
        // pointer arithmetic stays within `buf`.
        unsafe { find_byte_neon(buf, needle) }
    } else {
        find_byte_scalar(buf, needle)
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn count_byte(buf: &[u8], needle: u8) -> usize {
    if std::arch::is_aarch64_feature_detected!("neon") {
        // SAFETY: same as above.
        unsafe { count_byte_neon(buf, needle) }
    } else {
        count_byte_scalar(buf, needle)
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_byte_neon(buf: &[u8], needle: u8) -> Option<usize> {
    use std::arch::aarch64::*;

    let len = buf.len();
    let ptr = buf.as_ptr();
    let mut offset = 0usize;

    // SAFETY: `vdupq_n_u8` and `vceqq_u8` operate on 16-byte vectors.
    // We only load from `ptr.add(offset)` when `offset + 16 <= len`.
    let needle_vec = vdupq_n_u8(needle);

    while offset + 16 <= len {
        let chunk = vld1q_u8(ptr.add(offset));
        let cmp = vceqq_u8(chunk, needle_vec);

        // Reinterpret as two u64 lanes and check for any set bits.
        let lo = vgetq_lane_u64(vreinterpretq_u64_u8(cmp), 0);
        let hi = vgetq_lane_u64(vreinterpretq_u64_u8(cmp), 1);

        if lo != 0 {
            // Each matching byte becomes 0xFF; trailing_zeros / 8 gives byte index.
            return Some(offset + (lo.trailing_zeros() as usize / 8));
        }
        if hi != 0 {
            return Some(offset + 8 + (hi.trailing_zeros() as usize / 8));
        }

        offset += 16;
    }

    // Scalar tail for remaining bytes.
    for i in offset..len {
        if *ptr.add(i) == needle {
            return Some(i);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn count_byte_neon(buf: &[u8], needle: u8) -> usize {
    use std::arch::aarch64::*;

    let len = buf.len();
    let ptr = buf.as_ptr();
    let mut offset = 0usize;
    let mut total: usize = 0;

    let needle_vec = vdupq_n_u8(needle);

    while offset + 16 <= len {
        // SAFETY: same bounds guarantee as find_byte_neon.
        let chunk = vld1q_u8(ptr.add(offset));
        let cmp = vceqq_u8(chunk, needle_vec);

        // Each matching byte is 0xFF; shift right by 7 to get 0x01 per match,
        // then horizontally sum the 16 lanes to get the count for this chunk.
        let ones = vshrq_n_u8(cmp, 7);
        total += vaddvq_u8(ones) as usize;

        offset += 16;
    }

    // Scalar tail.
    for i in offset..len {
        if *ptr.add(i) == needle {
            total += 1;
        }
    }
    total
}

// ---------------------------------------------------------------------------
// x86_64 SSE2 implementation
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_byte(buf: &[u8], needle: u8) -> Option<usize> {
    if is_x86_feature_detected!("sse2") {
        // SAFETY: we have verified SSE2 support via runtime detection and all
        // pointer arithmetic stays within `buf`.
        unsafe { find_byte_sse2(buf, needle) }
    } else {
        find_byte_scalar(buf, needle)
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn count_byte(buf: &[u8], needle: u8) -> usize {
    if is_x86_feature_detected!("sse2") {
        // SAFETY: same as above.
        unsafe { count_byte_sse2(buf, needle) }
    } else {
        count_byte_scalar(buf, needle)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn find_byte_sse2(buf: &[u8], needle: u8) -> Option<usize> {
    use std::arch::x86_64::*;

    let len = buf.len();
    let ptr = buf.as_ptr();
    let mut offset = 0usize;

    // SAFETY: `_mm_set1_epi8` broadcasts `needle` into all 16 lanes.
    let needle_vec = _mm_set1_epi8(needle as i8);

    while offset + 16 <= len {
        // SAFETY: we only load when offset + 16 <= len.
        let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);
        let cmp = _mm_cmpeq_epi8(chunk, needle_vec);
        let mask = _mm_movemask_epi8(cmp) as u32;

        if mask != 0 {
            return Some(offset + mask.trailing_zeros() as usize);
        }

        offset += 16;
    }

    // Scalar tail.
    for i in offset..len {
        if *ptr.add(i) == needle {
            return Some(i);
        }
    }
    None
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn count_byte_sse2(buf: &[u8], needle: u8) -> usize {
    use std::arch::x86_64::*;

    let len = buf.len();
    let ptr = buf.as_ptr();
    let mut offset = 0usize;
    let mut total: usize = 0;

    let needle_vec = _mm_set1_epi8(needle as i8);

    while offset + 16 <= len {
        // SAFETY: we only load when offset + 16 <= len.
        let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);
        let cmp = _mm_cmpeq_epi8(chunk, needle_vec);
        let mask = _mm_movemask_epi8(cmp) as u32;
        total += mask.count_ones() as usize;

        offset += 16;
    }

    // Scalar tail.
    for i in offset..len {
        if *ptr.add(i) == needle {
            total += 1;
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Scalar fallback (all platforms)
// ---------------------------------------------------------------------------

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline]
fn find_byte(buf: &[u8], needle: u8) -> Option<usize> {
    find_byte_scalar(buf, needle)
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline]
fn count_byte(buf: &[u8], needle: u8) -> usize {
    count_byte_scalar(buf, needle)
}

#[inline]
fn find_byte_scalar(buf: &[u8], needle: u8) -> Option<usize> {
    buf.iter().position(|&b| b == needle)
}

#[inline]
fn count_byte_scalar(buf: &[u8], needle: u8) -> usize {
    buf.iter().filter(|&&b| b == needle).count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SOH: u8 = 0x01;

    // -- find_soh -----------------------------------------------------------

    #[test]
    fn find_soh_empty() {
        assert_eq!(find_soh(&[]), None);
    }

    #[test]
    fn find_soh_not_present() {
        let buf = b"8=FIX.4.4";
        assert_eq!(find_soh(buf), None);
    }

    #[test]
    fn find_soh_at_position_0() {
        let mut buf = [b'A'; 64];
        buf[0] = SOH;
        assert_eq!(find_soh(&buf), Some(0));
    }

    #[test]
    fn find_soh_at_position_1() {
        let mut buf = [b'A'; 64];
        buf[1] = SOH;
        assert_eq!(find_soh(&buf), Some(1));
    }

    #[test]
    fn find_soh_at_position_15() {
        let mut buf = [b'A'; 64];
        buf[15] = SOH;
        assert_eq!(find_soh(&buf), Some(15));
    }

    #[test]
    fn find_soh_at_position_16() {
        let mut buf = [b'A'; 64];
        buf[16] = SOH;
        assert_eq!(find_soh(&buf), Some(16));
    }

    #[test]
    fn find_soh_at_position_17() {
        let mut buf = [b'A'; 64];
        buf[17] = SOH;
        assert_eq!(find_soh(&buf), Some(17));
    }

    #[test]
    fn find_soh_at_position_31() {
        let mut buf = [b'A'; 64];
        buf[31] = SOH;
        assert_eq!(find_soh(&buf), Some(31));
    }

    #[test]
    fn find_soh_at_position_32() {
        let mut buf = [b'A'; 64];
        buf[32] = SOH;
        assert_eq!(find_soh(&buf), Some(32));
    }

    #[test]
    fn find_soh_at_middle() {
        let mut buf = [b'A'; 100];
        buf[50] = SOH;
        assert_eq!(find_soh(&buf), Some(50));
    }

    #[test]
    fn find_soh_at_end() {
        let mut buf = [b'A'; 64];
        buf[63] = SOH;
        assert_eq!(find_soh(&buf), Some(63));
    }

    #[test]
    fn find_soh_returns_first_of_multiple() {
        let mut buf = [b'A'; 64];
        buf[10] = SOH;
        buf[20] = SOH;
        buf[30] = SOH;
        assert_eq!(find_soh(&buf), Some(10));
    }

    #[test]
    fn find_soh_short_buffer() {
        let buf = [SOH];
        assert_eq!(find_soh(&buf), Some(0));

        let buf = [b'X', SOH];
        assert_eq!(find_soh(&buf), Some(1));

        let buf = [b'X'; 5];
        assert_eq!(find_soh(&buf), None);
    }

    #[test]
    fn find_soh_shorter_than_simd_width() {
        let mut buf = [b'A'; 10];
        buf[7] = SOH;
        assert_eq!(find_soh(&buf), Some(7));
    }

    // -- count_fields -------------------------------------------------------

    #[test]
    fn count_fields_empty() {
        assert_eq!(count_fields(&[]), 0);
    }

    #[test]
    fn count_fields_no_soh() {
        assert_eq!(count_fields(b"8=FIX.4.4"), 0);
    }

    #[test]
    fn count_fields_single() {
        let mut buf = [b'A'; 32];
        buf[10] = SOH;
        assert_eq!(count_fields(&buf), 1);
    }

    #[test]
    fn count_fields_multiple() {
        let mut buf = [b'A'; 64];
        buf[5] = SOH;
        buf[15] = SOH;
        buf[16] = SOH;
        buf[31] = SOH;
        buf[32] = SOH;
        buf[50] = SOH;
        assert_eq!(count_fields(&buf), 6);
    }

    #[test]
    fn count_fields_all_soh() {
        let buf = [SOH; 48];
        assert_eq!(count_fields(&buf), 48);
    }

    #[test]
    fn count_fields_short_buffer() {
        let buf = [SOH; 3];
        assert_eq!(count_fields(&buf), 3);
    }

    #[test]
    fn count_fields_realistic_fix_message() {
        // "8=FIX.4.4\x019=5\x0135=0\x0110=162\x01"
        let msg = b"8=FIX.4.4\x019=5\x0135=0\x0110=162\x01";
        assert_eq!(count_fields(msg), 4);
    }

    // -- find_equals --------------------------------------------------------

    #[test]
    fn find_equals_empty() {
        assert_eq!(find_equals(&[]), None);
    }

    #[test]
    fn find_equals_not_present() {
        assert_eq!(find_equals(b"hello"), None);
    }

    #[test]
    fn find_equals_at_start() {
        assert_eq!(find_equals(b"=value"), Some(0));
    }

    #[test]
    fn find_equals_typical_tag_value() {
        assert_eq!(find_equals(b"35=D"), Some(2));
    }

    #[test]
    fn find_equals_returns_first() {
        assert_eq!(find_equals(b"8=FIX=4.4"), Some(1));
    }

    #[test]
    fn find_equals_long_tag() {
        let mut buf = [b'9'; 40];
        buf[20] = b'=';
        assert_eq!(find_equals(&buf), Some(20));
    }

    // -- cross-check scalar vs SIMD -----------------------------------------

    #[test]
    fn find_byte_matches_scalar_sweep() {
        for size in 0..=130 {
            let mut buf = vec![b'X'; size];
            // No needle present.
            assert_eq!(find_soh(&buf), find_byte_scalar(&buf, SOH));
            assert_eq!(count_fields(&buf), count_byte_scalar(&buf, SOH));

            // Place needle at every valid position.
            for pos in 0..size {
                buf[pos] = SOH;
                assert_eq!(
                    find_soh(&buf),
                    find_byte_scalar(&buf, SOH),
                    "find_soh mismatch at size={size} pos={pos}"
                );
                assert_eq!(
                    count_fields(&buf),
                    count_byte_scalar(&buf, SOH),
                    "count_fields mismatch at size={size} pos={pos}"
                );
                buf[pos] = b'X';
            }
        }
    }
}

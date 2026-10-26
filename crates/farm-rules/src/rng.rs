//! A small deterministic generator (SplitMix64), so a save's seed and day replay the same weather.

/// One SplitMix64 step of `state`.
pub fn mix(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A value in `0..n` drawn from `seed` and `salt`.
pub fn below(seed: u64, salt: u64, n: u64) -> u64 {
    mix(seed ^ mix(salt)) % n.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_are_deterministic_and_in_range() {
        for salt in 0..100 {
            let a = below(42, salt, 7);
            assert_eq!(a, below(42, salt, 7));
            assert!(a < 7);
        }
        assert_ne!(mix(1), mix(2));
    }
}

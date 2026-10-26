//! SplitMix64, seeded from `Command::Start`: role and task draws replay exactly from the seed, with
//! no OS randomness (wasm-safe and testable).

#[derive(Clone, Debug)]
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// An index in `0..bound` by multiply-shift (Lemire); the bias is below `bound / 2^64`.
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        ((u128::from(self.next_u64()) * bound as u128) >> 64) as usize
    }

    /// Fisher-Yates.
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_splitmix64() {
        // Vigna's splitmix64.c from state 0.
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn shuffle_is_a_seeded_permutation() {
        let shuffled = |seed| {
            let mut v: Vec<u32> = (0..10).collect();
            SplitMix64::new(seed).shuffle(&mut v);
            v
        };
        let a = shuffled(7);
        assert_eq!(a, shuffled(7));
        assert_ne!(a, shuffled(8));
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn below_stays_in_bounds() {
        let mut rng = SplitMix64::new(1);
        assert!((0..1000).all(|_| rng.below(3) < 3));
    }
}

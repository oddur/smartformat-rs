//! A seeded pseudo-random generator, written out rather than pulled in.
//!
//! A campaign's whole value rests on `--seed N` reproducing it exactly, and a
//! dependency's generator is free to change its stream between versions — the
//! `rand` crate has done it more than once. `xoshiro256**` seeded through
//! `SplitMix64` is thirty lines, is fixed here for good, and reproduces a run
//! byte for byte on any machine and any compiler.

/// `xoshiro256**`, seeded from a single `u64` through `SplitMix64`.
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            x ^ (x >> 31)
        };
        Self {
            state: [next(), next(), next(), next()],
        }
    }

    /// Derives an independent generator from this one's seed and a label, so
    /// case *n* of a campaign is built from a stream that does not depend on
    /// how many random numbers case *n − 1* happened to draw.
    pub fn derive(seed: u64, label: u64) -> Self {
        let mut mix = seed ^ label.wrapping_mul(0xd6e8_feb8_6659_fd93);
        mix ^= mix >> 32;
        Self::new(mix.wrapping_mul(0xff51_afd7_ed55_8ccd))
    }

    pub fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.state;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s1 << 17;
        let mut s = [s0 ^ s3, s1 ^ s2, s2 ^ s0, s3 ^ s1];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        self.state = s;
        result
    }

    /// A value in `0..limit`. `limit` of zero is zero.
    pub fn below(&mut self, limit: usize) -> usize {
        if limit <= 1 {
            return 0;
        }
        // Lemire's multiply-shift, without the rejection loop: the bias is
        // below 2^-64 relative for every limit a generator here uses.
        ((self.next_u64() as u128 * limit as u128) >> 64) as usize
    }

    /// An inclusive range.
    pub fn range(&mut self, low: i64, high: i64) -> i64 {
        debug_assert!(low <= high);
        low + self.below((high - low + 1) as usize) as i64
    }

    /// True with the given percentage chance.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent as usize
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// Picks an index in proportion to its weight. All-zero weights pick 0.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u32 = weights.iter().sum();
        if total == 0 {
            return 0;
        }
        let mut roll = self.below(total as usize) as u32;
        for (index, weight) in weights.iter().enumerate() {
            if roll < *weight {
                return index;
            }
            roll -= *weight;
        }
        weights.len() - 1
    }

    /// Picks one of `(weight, value)` pairs in proportion to the weights.
    pub fn weighted_pick<'a, T>(&mut self, table: &'a [(u32, T)]) -> &'a T {
        let weights: Vec<u32> = table.iter().map(|(weight, _)| *weight).collect();
        &table[self.weighted(&weights)].1
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn a_seed_reproduces_its_stream() {
        let first: Vec<u64> = (0..16).map(|_| Rng::new(7).next_u64()).collect();
        let mut generator = Rng::new(7);
        let second: Vec<u64> = (0..16).map(|_| generator.next_u64()).collect();
        assert_eq!(first[0], second[0]);

        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn derived_streams_differ() {
        let mut a = Rng::derive(1, 0);
        let mut b = Rng::derive(1, 1);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn ranges_stay_inside_their_bounds() {
        let mut generator = Rng::new(99);
        for _ in 0..10_000 {
            let value = generator.range(-12, 12);
            assert!((-12..=12).contains(&value));
            assert!(generator.below(5) < 5);
        }
    }

    #[test]
    fn weights_of_zero_are_never_picked() {
        let mut generator = Rng::new(3);
        for _ in 0..1000 {
            assert_ne!(generator.weighted(&[0, 1, 0]), 0);
            assert_ne!(generator.weighted(&[0, 1, 0]), 2);
        }
    }
}

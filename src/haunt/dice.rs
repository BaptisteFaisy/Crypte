//! Dé déterministe (xorshift64) : même graine, même partie (tests).

#[derive(Debug, Clone)]
pub struct Dice(u64);

impl Dice {
    /// Dé de graine `seed` (0 remplacé par 1, xorshift exige non nul).
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// Entier pseudo-aléatoire suivant.
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Entier dans `0..n` (0 si `n` vaut 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }

    /// Flottant dans `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        lo + (hi - lo) * unit
    }

    /// -1 ou +1, à pile ou face.
    pub fn sign(&mut self) -> f32 {
        if self.next() & 1 == 0 {
            -1.0
        } else {
            1.0
        }
    }
}

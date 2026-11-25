//! Deterministic replay of Showdown's Gen 5 and Sodium seeds.
//! Based on Pokémon Showdown's sim/prng.ts (MIT, see docs/LICENSE.pokemon-showdown).

use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    Gen5([u16; 4]),
    Sodium([u8; 32]),
}

impl Seed {
    /// Showdown seeds a new Sodium PRNG with 128 random bits, zero-padded
    /// to the 32-byte key. Explicit replay seeds never use system entropy.
    pub fn from_entropy() -> std::io::Result<Self> {
        use std::io::Read;
        let mut key = [0; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut key[..16])?;
        Ok(Self::Sodium(key))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSeed;

impl fmt::Display for InvalidSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected four decimal u16 words, gen5,<16 hex digits>, or sodium,<up to 64 hex digits>")
    }
}

impl std::error::Error for InvalidSeed {}

impl FromStr for Seed {
    type Err = InvalidSeed;

    fn from_str(seed: &str) -> Result<Self, Self::Err> {
        if let Some(hex) = seed.strip_prefix("sodium,") {
            if hex.len() > 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(InvalidSeed);
            }
            let mut key = [0; 32];
            for (i, digit) in hex.bytes().enumerate() {
                let digit = (digit as char).to_digit(16).ok_or(InvalidSeed)? as u8;
                key[i / 2] |= if i % 2 == 0 { digit << 4 } else { digit };
            }
            return Ok(Self::Sodium(key));
        }
        let mut words = [0; 4];
        if let Some(hex) = seed.strip_prefix("gen5,") {
            if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(InvalidSeed);
            }
            for (i, word) in words.iter_mut().enumerate() {
                *word =
                    u16::from_str_radix(&hex[i * 4..(i + 1) * 4], 16).map_err(|_| InvalidSeed)?;
            }
        } else {
            let mut decimal = seed.split(',');
            for word in &mut words {
                *word = decimal
                    .next()
                    .ok_or(InvalidSeed)?
                    .parse()
                    .map_err(|_| InvalidSeed)?;
            }
            if decimal.next().is_some() {
                return Err(InvalidSeed);
            }
        }
        Ok(Self::Gen5(words))
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gen5([a, b, c, d]) => write!(f, "{a},{b},{c},{d}"),
            Self::Sodium(key) => {
                f.write_str("sodium,")?;
                for byte in key {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng(Seed);

impl Rng {
    pub fn new(seed: Seed) -> Self {
        Self(seed)
    }

    pub fn seed(&self) -> Seed {
        self.0
    }

    pub fn next_word(&mut self) -> u32 {
        match &mut self.0 {
            Seed::Gen5(seed) => {
                let [a, b, c, d] = seed.map(u32::from);
                // JS's >>> 16 discards overflow past bit 32 at each limb. A
                // single u64 multiply does not reproduce that for every seed.
                let w = d * 0x8965 + 0x9ec3;
                let x = c * 0x8965 + d * 0x6c07 + 0x26 + (w >> 16);
                let y = (b * 0x8965 + c * 0x6c07)
                    .wrapping_add(d * 0x8b65)
                    .wrapping_add(x >> 16);
                let z = (a * 0x8965 + b * 0x6c07)
                    .wrapping_add(c * 0x8b65)
                    .wrapping_add(d * 0x5d58)
                    .wrapping_add(y >> 16);
                *seed = [z as u16, y as u16, x as u16, w as u16];
                (z << 16) | (y & 0xffff)
            }
            Seed::Sodium(key) => {
                let mut block = [0; 36];
                ChaCha20::new((&*key).into(), b"LibsodiumDRG".into()).apply_keystream(&mut block);
                key.copy_from_slice(&block[..32]);
                u32::from_be_bytes([block[32], block[33], block[34], block[35]])
            }
        }
    }

    /// Draw from 0..upper. Even an upper bound of 0 or 1 consumes one word,
    /// just as Showdown does.
    pub fn below(&mut self, upper: u32) -> u32 {
        let word = self.next_word();
        if upper <= 1 << 21 {
            ((u64::from(word) * u64::from(upper)) >> 32) as u32
        } else {
            (f64::from(word) * f64::from(upper) / 4294967296.0) as u32
        }
    }

    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        self.below(denominator) < numerator
    }

    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        let len = u32::try_from(list.len()).expect("shuffle length exceeds u32");
        for i in 0..len.saturating_sub(1) {
            let j = i + self.below(len - i);
            list.swap(i as usize, j as usize);
        }
    }
}

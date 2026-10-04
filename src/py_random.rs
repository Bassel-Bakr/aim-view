//! Python's `random.Random` seeded with a string (CPython's Mersenne Twister, seeded from the string and its SHA-512),
//! and MD5 as hashlib gives it.
//!
//! In: a seed string, or bytes to hash. Out: the same numbers and digests Python gives. The faint-target cut-off's
//! labels pick their crops and name their files with them, as python/model/hand_crops.py does (src/faint.rs:
//! `cutoff_crops`). The twister follows CPython's Modules/_randommodule.c, MD5 RFC 1321 and SHA-512 FIPS 180-4, step
//! for step.

/// The twister's state, in 32-bit words (`N` in _randommodule.c).
const STATE_WORDS: usize = 624;
/// The word a twist mixes in, this far after the one it changes (`M`).
const TWIST_OFFSET: usize = 397;
/// The twist's matrix, applied when the joined word is odd (`MATRIX_A`).
const TWIST_MATRIX: u32 = 0x9908_b0df;
/// The top bit, and the bits below it: a twist joins one word's top bit with the next word's others (`UPPER_MASK`,
/// `LOWER_MASK`).
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;
/// `init_by_array`'s seed for `init_genrand`, before the key is mixed in.
const INIT_BY_ARRAY_SEED: u32 = 19_650_218;
/// The multipliers of `init_genrand`, and of `init_by_array`'s two passes over the state.
const INIT_MULTIPLIER: u32 = 1_812_433_253;
const KEY_PASS_MULTIPLIER: u32 = 1_664_525;
const FINAL_PASS_MULTIPLIER: u32 = 1_566_083_941;
/// `genrand_uint32`'s tempering masks.
const TEMPER_MASK_B: u32 = 0x9d2c_5680;
const TEMPER_MASK_C: u32 = 0xefc6_0000;
/// Bits in a draw of the twister.
const DRAW_BITS: u32 = 32;

/// `random.Random(seed)` for a string seed: the same numbers as Python gives, call for call.
pub struct PyRandom {
    /// The twister's state (`mt` in _randommodule.c).
    mt: [u32; STATE_WORDS],
    /// The next word to temper and give; `STATE_WORDS` when the state must twist first.
    index: usize,
}

impl PyRandom {
    /// Python's `random.Random(seed)` (version 2): the seed is the integer whose big-endian bytes are the string's
    /// UTF-8 bytes followed by their SHA-512, given to the twister as 32-bit words, least significant first.
    pub fn seeded(seed: &str) -> PyRandom {
        let bytes = [seed.as_bytes(), &sha512(seed.as_bytes())].concat();
        let start = bytes.iter().position(|&byte| byte != 0).unwrap_or(bytes.len());
        let little_endian: Vec<u8> = bytes[start..].iter().rev().copied().collect();
        let mut key: Vec<u32> = little_endian
            .chunks(4)
            .map(|word_bytes| {
                word_bytes.iter().enumerate().fold(0u32, |word, (place, &byte)| word | (byte as u32) << (8 * place))
            })
            .collect();
        if key.is_empty() {
            key.push(0);
        }
        PyRandom::from_key(&key)
    }

    /// The twister's `init_by_array`.
    fn from_key(key: &[u32]) -> PyRandom {
        let mut mt = [0u32; STATE_WORDS];
        mt[0] = INIT_BY_ARRAY_SEED;
        for i in 1..STATE_WORDS {
            mt[i] = INIT_MULTIPLIER.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..STATE_WORDS.max(key.len()) {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(KEY_PASS_MULTIPLIER))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= STATE_WORDS {
                mt[0] = mt[STATE_WORDS - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..STATE_WORDS - 1 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(FINAL_PASS_MULTIPLIER))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= STATE_WORDS {
                mt[0] = mt[STATE_WORDS - 1];
                i = 1;
            }
        }
        // a non-zero state, whatever the key
        mt[0] = UPPER_MASK;
        PyRandom { mt, index: STATE_WORDS }
    }

    /// The whole state twisted once, word by word in place, as `genrand_uint32` does when it has used every word.
    fn twist(&mut self) {
        for i in 0..STATE_WORDS {
            let joined = (self.mt[i] & UPPER_MASK) | (self.mt[(i + 1) % STATE_WORDS] & LOWER_MASK);
            let matrix = if joined & 1 == 1 { TWIST_MATRIX } else { 0 };
            self.mt[i] = self.mt[(i + TWIST_OFFSET) % STATE_WORDS] ^ (joined >> 1) ^ matrix;
        }
        self.index = 0;
    }

    /// The twister's next 32 bits (`genrand_uint32`): the next word of the state, tempered.
    fn next_u32(&mut self) -> u32 {
        if self.index >= STATE_WORDS {
            self.twist();
        }
        let mut tempered = self.mt[self.index];
        self.index += 1;
        tempered ^= tempered >> 11;
        tempered ^= (tempered << 7) & TEMPER_MASK_B;
        tempered ^= (tempered << 15) & TEMPER_MASK_C;
        tempered ^ (tempered >> 18)
    }

    /// `_randbelow(n)`: a number from 0 to `bound` - 1, drawing as many bits as `bound` has until one is below it.
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0, "below(0)");
        let bits = DRAW_BITS - bound.leading_zeros();
        loop {
            let draw = self.next_u32() >> (DRAW_BITS - bits);
            if draw < bound {
                return draw;
            }
        }
    }

    /// `randint(a, b)`: a number from a to b, both included.
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        a + self.below((b - a + 1) as u32) as i64
    }

    /// `choice(seq)`'s pick: an index into a sequence of `len` items.
    pub fn choice(&mut self, len: usize) -> usize {
        self.below(len as u32) as usize
    }
}

/// SHA-512's message block, in bytes.
const SHA512_BLOCK_BYTES: usize = 128;
/// SHA-512's rounds a block, one word of the message schedule each.
const SHA512_ROUNDS: usize = 80;
/// The schedule's words read straight from a block; the rest are mixed from them.
const SHA512_BLOCK_WORDS: usize = 16;

/// SHA-512's round constants (FIPS 180-4, 4.2.3: `K`).
const SHA512_ROUND_CONSTANTS: [u64; SHA512_ROUNDS] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538,
    0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118, 0xd807aa98a3030242, 0x12835b0145706fbe,
    0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235,
    0xc19bf174cf692694, 0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5, 0x983e5152ee66dfab,
    0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725,
    0x06ca6351e003826f, 0x142929670a0e6e70, 0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218,
    0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8, 0x19a4c116b8d2d0c8, 0x1e376c085141ab53,
    0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3, 0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b, 0xca273eceea26619c,
    0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6,
    0x113f9804bef90dae, 0x1b710b35131c471b, 0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// SHA-512's hash before the first block (FIPS 180-4, 5.3.5: `H(0)`).
const SHA512_INITIAL_HASH: [u64; 8] = [
    0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1,
    0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
];

/// SHA-512 of the bytes.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let length_bits = ((data.len() as u128) * 8).to_be_bytes();
    let mut hash = SHA512_INITIAL_HASH;
    for block in padded(data, SHA512_BLOCK_BYTES, &length_bits).chunks_exact(SHA512_BLOCK_BYTES) {
        sha512_block(&mut hash, block);
    }
    let mut digest = [0u8; 64];
    for (word_index, word) in hash.iter().enumerate() {
        digest[8 * word_index..8 * word_index + 8].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// One block into the hash (FIPS 180-4, 6.4.2). The working variables a to h are `work[0]` to `work[7]`.
fn sha512_block(hash: &mut [u64; 8], block: &[u8]) {
    let schedule = sha512_schedule(block);
    let mut work = *hash;
    for round in 0..SHA512_ROUNDS {
        let temp1 = work[7]
            .wrapping_add(big_sigma1(work[4]))
            .wrapping_add(choose(work[4], work[5], work[6]))
            .wrapping_add(SHA512_ROUND_CONSTANTS[round])
            .wrapping_add(schedule[round]);
        let temp2 = big_sigma0(work[0]).wrapping_add(majority(work[0], work[1], work[2]));
        // h takes g, g takes f, and so on to b, which takes a; then e is d plus temp1, and a is temp1 plus temp2
        work.rotate_right(1);
        work[4] = work[4].wrapping_add(temp1);
        work[0] = temp1.wrapping_add(temp2);
    }
    for (word, worked) in hash.iter_mut().zip(work) {
        *word = word.wrapping_add(worked);
    }
}

/// A block's message schedule (FIPS 180-4, 6.4.2 step 1: `W`).
fn sha512_schedule(block: &[u8]) -> [u64; SHA512_ROUNDS] {
    let mut schedule = [0u64; SHA512_ROUNDS];
    for round in 0..SHA512_BLOCK_WORDS {
        schedule[round] = u64::from_be_bytes(block[8 * round..8 * round + 8].try_into().unwrap());
    }
    for round in SHA512_BLOCK_WORDS..SHA512_ROUNDS {
        schedule[round] = schedule[round - 16]
            .wrapping_add(small_sigma0(schedule[round - 15]))
            .wrapping_add(schedule[round - 7])
            .wrapping_add(small_sigma1(schedule[round - 2]));
    }
    schedule
}

/// FIPS 180-4's `Ch`: each bit from `if_set` where the selector's bit is set, else from `if_clear`.
fn choose(selector: u64, if_set: u64, if_clear: u64) -> u64 {
    (selector & if_set) ^ (!selector & if_clear)
}

/// FIPS 180-4's `Maj`: each bit as most of the three words have it.
fn majority(first: u64, second: u64, third: u64) -> u64 {
    (first & second) ^ (first & third) ^ (second & third)
}

/// FIPS 180-4's `Σ0`, on the working variable a.
fn big_sigma0(word: u64) -> u64 {
    word.rotate_right(28) ^ word.rotate_right(34) ^ word.rotate_right(39)
}

/// FIPS 180-4's `Σ1`, on the working variable e.
fn big_sigma1(word: u64) -> u64 {
    word.rotate_right(14) ^ word.rotate_right(18) ^ word.rotate_right(41)
}

/// FIPS 180-4's `σ0` and `σ1`, in the message schedule.
fn small_sigma0(word: u64) -> u64 {
    word.rotate_right(1) ^ word.rotate_right(8) ^ (word >> 7)
}

fn small_sigma1(word: u64) -> u64 {
    word.rotate_right(19) ^ word.rotate_right(61) ^ (word >> 6)
}

/// The message as MD5 and SHA-512 pad it: a 1 bit, zeros up to the place of its length in the last block, then the
/// length (`length`, as the hash writes it).
fn padded(data: &[u8], block_bytes: usize, length: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity((data.len() + 1 + length.len()).div_ceil(block_bytes) * block_bytes);
    message.extend_from_slice(data);
    message.push(0x80);
    while message.len() % block_bytes != block_bytes - length.len() {
        message.push(0);
    }
    message.extend_from_slice(length);
    message
}

/// MD5's message block, in bytes and in words, and its steps a block (four rounds of 16).
const MD5_BLOCK_BYTES: usize = 64;
const MD5_BLOCK_WORDS: usize = 16;
const MD5_STEPS: usize = 64;
const MD5_STEPS_A_ROUND: usize = 16;

/// How far each step turns its sum left (RFC 1321, 3.4: `S11` to `S44`).
const MD5_SHIFTS: [u32; MD5_STEPS] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
    21,
];

/// The constant each step adds, from the sine (RFC 1321, 3.4: `T`).
const MD5_SINES: [u32; MD5_STEPS] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a,
    0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
    0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
    0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8,
    0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
    0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
    0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
    0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// MD5's state before the first block (RFC 1321, 3.3: words A, B, C and D).
const MD5_INITIAL_STATE: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

/// MD5 of the bytes.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let length_bits = (data.len() as u64).wrapping_mul(8).to_le_bytes();
    let mut state = MD5_INITIAL_STATE;
    for block in padded(data, MD5_BLOCK_BYTES, &length_bits).chunks_exact(MD5_BLOCK_BYTES) {
        md5_block(&mut state, block);
    }
    let mut digest = [0u8; 16];
    for (word_index, word) in state.iter().enumerate() {
        digest[4 * word_index..4 * word_index + 4].copy_from_slice(&word.to_le_bytes());
    }
    digest
}

/// One block into the state (RFC 1321, 3.4). The working words a, b, c and d are `work[0]` to `work[3]`.
fn md5_block(state: &mut [u32; 4], block: &[u8]) {
    let words: [u32; MD5_BLOCK_WORDS] =
        std::array::from_fn(|i| u32::from_le_bytes(block[4 * i..4 * i + 4].try_into().unwrap()));
    let mut work = *state;
    for step in 0..MD5_STEPS {
        let (mixed, word_index) = md5_step(step, &work);
        let sum = mixed.wrapping_add(work[0]).wrapping_add(MD5_SINES[step]).wrapping_add(words[word_index]);
        // a takes d, d takes c, c takes b, and b is b plus the sum turned left
        work = [work[3], work[1].wrapping_add(sum.rotate_left(MD5_SHIFTS[step])), work[1], work[2]];
    }
    for (word, worked) in state.iter_mut().zip(work) {
        *word = word.wrapping_add(worked);
    }
}

/// A step's round function (RFC 1321's F, G, H or I) of the working words b, c and d, and the index of the block's
/// word the step adds.
fn md5_step(step: usize, work: &[u32; 4]) -> (u32, usize) {
    let (b_word, c_word, d_word) = (work[1], work[2], work[3]);
    match step / MD5_STEPS_A_ROUND {
        0 => ((b_word & c_word) | (!b_word & d_word), step),
        1 => ((d_word & b_word) | (!d_word & c_word), (5 * step + 1) % MD5_BLOCK_WORDS),
        2 => (b_word ^ c_word ^ d_word, (3 * step + 5) % MD5_BLOCK_WORDS),
        _ => (c_word ^ (b_word | !d_word), (7 * step) % MD5_BLOCK_WORDS),
    }
}

/// Bytes as lower-case hex, as `hexdigest()` gives them.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hashes_give_the_known_digests() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd\
             454d4423643ce80e2a9ac94fa54ca49f"
        );
        let long = vec![b'a'; 200];
        assert_eq!(
            hex(&sha512(&long)),
            "4b11459c33f52a22ee8236782714c150a3b2c60994e9acee17fe68947a3e6789f31e7668394592da7bef827cddca88c4\
             e6f86e4df7ed1ae6cba71f3e98faee9f"
        );
    }

    /// Python 3.14: `r = random.Random("seed")`, then `[r.randint(-48, 48) for _ in range(5)]`,
    /// `r.choice(range(7))` and `r.getrandbits(32)`.
    #[test]
    fn a_string_seed_gives_pythons_numbers() {
        let mut random = PyRandom::seeded("seed");
        let got: Vec<i64> = (0..5).map(|_| random.randint(-48, 48)).collect();
        assert_eq!(got, PYTHON_RANDINTS);
        assert_eq!(random.choice(7), PYTHON_CHOICE);
        assert_eq!(random.next_u32(), PYTHON_BITS);
    }

    const PYTHON_RANDINTS: [i64; 5] = [-31, -41, -18, -4, -12];
    const PYTHON_CHOICE: usize = 3;
    const PYTHON_BITS: u32 = 185764099;
}

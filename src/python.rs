//! Arithmetic done the way Python and NumPy do it, where the last bit of a result can reach a report: rounding to a
//! number of decimals, CPython's `math.hypot`, and NumPy's sums, means, medians and percentiles.
//!
//! In: plain floats. Out: Python's results to the bit, for the steps the core ports from Python (the tracks' rounding
//! and view shift in track.rs, the pop-up watch, the matching, the measures, the tracking summary, the area finder,
//! the mouse log and the faint cut-off). Each function follows its C source step for step: change none of the order.

/// The fraction's bits in an IEEE 754 double, below the exponent.
const FRACTION_BITS: u32 = 52;
/// The 11 exponent bits of a double, once shifted down past the fraction.
const EXPONENT_MASK: u64 = 0x7ff;
/// The bias of a double's exponent: the stored exponent less this is the power of two.
const EXPONENT_BIAS: i32 = 1023;
/// The sign and exponent bits above the fraction in a double's 64.
const SIGN_AND_EXPONENT_BITS: i32 = 12;
/// `frexp` gives a mantissa from 0.5 up to 1, one power of two under IEEE's from 1 up to 2: its exponent is the
/// biased exponent minus this.
const FREXP_BIAS: i32 = 1022;
/// NumPy's pairwise sum adds this many values one by one, and in its unrolled loop keeps this many partial sums.
const PAIRWISE_UNROLL: usize = 8;
/// NumPy's pairwise sum splits a longer array in two (`PW_BLOCKSIZE`).
const PAIRWISE_BLOCK: usize = 128;

/// Python's `round(x, digits)`: the decimal nearest the exact value of `x`, ties to even.
pub fn round(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// Python's `math.hypot(x, y)`: CPython's `vector_norm` (Modules/mathmodule.c, 3.14), which squares exactly, sums
/// with compensation and corrects the square root once, so it is usually correctly rounded. The C library's `hypot`
/// (Rust's `f64::hypot`, and NumPy's `np.hypot`) can differ from it in the last bit.
pub fn hypot(x: f64, y: f64) -> f64 {
    let magnitudes = [x.abs(), y.abs()];
    let max = magnitudes[0].max(magnitudes[1]);
    if magnitudes.iter().any(|a| a.is_nan()) {
        return if max.is_infinite() { max } else { f64::NAN };
    }
    if max.is_infinite() || max == 0.0 {
        return max;
    }
    vector_norm(magnitudes, max)
}

/// CPython's `vector_norm` for two finite magnitudes, the largest `max` (not 0): scaled so `max` is near 1, squared and
/// summed with the rounding errors kept apart, then the square root corrected once by the residual.
fn vector_norm(magnitudes: [f64; 2], max: f64) -> f64 {
    let max_exponent = frexp_exponent(max);
    if max_exponent < -EXPONENT_BIAS {
        // subnormal: scaled up to normals first, as CPython does
        let tiny = f64::MIN_POSITIVE;
        return tiny * vector_norm([magnitudes[0] / tiny, magnitudes[1] / tiny], max / tiny);
    }
    // ldexp(1.0, -max_exponent)
    let scale = f64::from_bits(((EXPONENT_BIAS - max_exponent) as u64) << FRACTION_BITS);
    let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
    for magnitude in magnitudes {
        let x = magnitude * scale;
        let (high, low) = exact_product(x, x);
        let (sum, error) = exact_sum(csum, high);
        csum = sum;
        frac1 += low;
        frac2 += error;
    }
    let mut norm = (csum - 1.0 + (frac1 + frac2)).sqrt();
    let (high, low) = exact_product(-norm, norm);
    let (sum, error) = exact_sum(csum, high);
    csum = sum;
    frac1 += low;
    frac2 += error;
    let residual = csum - 1.0 + (frac1 + frac2);
    norm += residual / (2.0 * norm);
    norm / scale
}

/// The exponent C's `frexp` gives: x = m * 2^e with 0.5 <= m < 1.
fn frexp_exponent(x: f64) -> i32 {
    let biased = (x.to_bits() >> FRACTION_BITS) & EXPONENT_MASK;
    if biased == 0 {
        // subnormal: count from the highest set bit of the fraction
        let fraction = x.to_bits() & ((1u64 << FRACTION_BITS) - 1);
        -FREXP_BIAS - (fraction.leading_zeros() as i32 - SIGN_AND_EXPONENT_BITS)
    } else {
        biased as i32 - FREXP_BIAS
    }
}

/// An exact product (CPython's `dl_mul`): high + low == x * y.
fn exact_product(x: f64, y: f64) -> (f64, f64) {
    let high = x * y;
    (high, x.mul_add(y, -high))
}

/// An exact sum (CPython's `dl_fast_sum`), for |a| >= |b|: sum + error == a + b.
fn exact_sum(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    (sum, (a - sum) + b)
}

/// NumPy's sum of a float64 array (`np.add.reduce`): pairwise, in blocks of eight, so a long sum can differ in the last
/// bit from adding the values one by one.
pub fn numpy_sum(values: &[f64]) -> f64 {
    0.0 + pairwise_sum(values)
}

/// NumPy's mean of a float64 array: its sum, divided by the count.
pub fn numpy_mean(values: &[f64]) -> f64 {
    numpy_sum(values) / values.len() as f64
}

/// NumPy's `percentile(a, q)` with its default "linear" method: the value at (count - 1) * percent / 100 in sorted
/// order, interpolated from both sides the way `_lerp` does it. NaN for no values.
pub fn numpy_percentile(values: &[f64], percent: f64) -> f64 {
    let count = values.len();
    if count == 0 {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let virtual_index = (count - 1) as f64 * (percent / 100.0);
    if virtual_index >= (count - 1) as f64 {
        return sorted[count - 1];
    }
    if virtual_index < 0.0 {
        return sorted[0];
    }
    let below = virtual_index.floor() as usize;
    lerp(sorted[below], sorted[below + 1], virtual_index - below as f64)
}

/// NumPy's `_lerp`: from the near end, so the result never leaves [a, b]. `weight` is how far from a to b.
fn lerp(a: f64, b: f64, weight: f64) -> f64 {
    let diff = b - a;
    if weight >= 0.5 { b - diff * (1.0 - weight) } else { a + diff * weight }
}

/// NumPy's `median`: the middle value, or the mean of the two middle values. NaN for no values.
pub fn numpy_median(values: &[f64]) -> f64 {
    let count = values.len();
    if count == 0 {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = count / 2;
    if count % 2 == 1 { sorted[middle] } else { numpy_mean(&sorted[middle - 1..=middle]) }
}

/// NumPy's `pairwise_sum` (numpy/_core/src/umath/loops_utils.h.src).
fn pairwise_sum(values: &[f64]) -> f64 {
    let count = values.len();
    if count < PAIRWISE_UNROLL {
        let mut sum = 0.0;
        for value in values {
            sum += value;
        }
        sum
    } else if count <= PAIRWISE_BLOCK {
        unrolled_sum(values)
    } else {
        let mut half = count / 2;
        half -= half % PAIRWISE_UNROLL;
        pairwise_sum(&values[..half]) + pairwise_sum(&values[half..])
    }
}

/// `pairwise_sum`'s unrolled loop: eight partial sums over the values eight at a time, added in pairs, then the
/// values left over one by one.
fn unrolled_sum(values: &[f64]) -> f64 {
    let count = values.len();
    let mut partial: [f64; PAIRWISE_UNROLL] = std::array::from_fn(|lane| values[lane]);
    let mut i = PAIRWISE_UNROLL;
    while i < count - count % PAIRWISE_UNROLL {
        for (j, lane) in partial.iter_mut().enumerate() {
            *lane += values[i + j];
        }
        i += PAIRWISE_UNROLL;
    }
    let mut sum = ((partial[0] + partial[1]) + (partial[2] + partial[3]))
        + ((partial[4] + partial[5]) + (partial[6] + partial[7]));
    while i < count {
        sum += values[i];
        i += 1;
    }
    sum
}

/// Checks rounding and sums against values Python gives.
#[cfg(test)]
mod tests {
    use super::*;

    /// `round` rounds the stored value, not the decimal it was typed as, and ties go to even.
    #[test]
    fn rounds_as_python_does() {
        assert_eq!(round(23.00034999, 4), 23.0003);
        assert_eq!(round(0.125, 2), 0.12);
        assert_eq!(round(-4.86225, 4), -4.8623); // stored as -4.862250000000000405: past the tie
        assert_eq!(round(2.5, 0), 2.0);
    }

    /// A pairwise sum of 300 values stays within 1e-9 of adding them one by one, and short sums and means are exact.
    #[test]
    fn sums_in_numpys_order() {
        let a: Vec<f64> = (0..300).map(|i| 0.1 * i as f64 + 1e-9 * (i * i) as f64).collect();
        let by_one: f64 = a.iter().sum();
        assert!((numpy_sum(&a) - by_one).abs() < 1e-9);
        assert_eq!(numpy_sum(&[1.0, 2.0, 3.0]), 6.0);
        assert_eq!(numpy_mean(&[1.0, 2.0]), 1.5);
    }
}

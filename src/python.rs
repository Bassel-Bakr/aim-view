//! Arithmetic done the way Python and NumPy do it, where the last bit of a result can reach a report: rounding to a
//! number of decimals, and NumPy's sums and means.

/// Python's `round(x, digits)`: the decimal nearest the exact value of `x`, ties to even.
pub fn round(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// Python's `math.hypot(x, y)`: CPython's `vector_norm` (Modules/mathmodule.c, 3.14), which squares exactly, sums
/// with compensation and corrects the square root once, so it is usually correctly rounded. The C library's `hypot`
/// (Rust's `f64::hypot`, and NumPy's `np.hypot`) can differ from it in the last bit.
pub fn hypot(x: f64, y: f64) -> f64 {
    let v = [x.abs(), y.abs()];
    let max = v[0].max(v[1]);
    if v.iter().any(|a| a.is_nan()) {
        return if max.is_infinite() { max } else { f64::NAN };
    }
    if max.is_infinite() || max == 0.0 {
        return max;
    }
    vector_norm(v, max)
}

fn vector_norm(vec: [f64; 2], max: f64) -> f64 {
    let max_e = frexp_exponent(max);
    if max_e < -1023 {
        // subnormal: scaled up to normals first, as CPython does
        let tiny = f64::MIN_POSITIVE;
        return tiny * vector_norm([vec[0] / tiny, vec[1] / tiny], max / tiny);
    }
    let scale = f64::from_bits(((1023 - max_e) as u64) << 52); // ldexp(1.0, -max_e)
    let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
    for x in vec {
        let x = x * scale;
        let (hi, lo) = dl_mul(x, x);
        let (s, e) = dl_fast_sum(csum, hi);
        csum = s;
        frac1 += lo;
        frac2 += e;
    }
    let mut h = (csum - 1.0 + (frac1 + frac2)).sqrt();
    let (hi, lo) = dl_mul(-h, h);
    let (s, e) = dl_fast_sum(csum, hi);
    csum = s;
    frac1 += lo;
    frac2 += e;
    let x = csum - 1.0 + (frac1 + frac2);
    h += x / (2.0 * h);
    h / scale
}

/// The exponent C's `frexp` gives: x = m * 2^e with 0.5 <= m < 1.
fn frexp_exponent(x: f64) -> i32 {
    let bits = (x.to_bits() >> 52) & 0x7ff;
    if bits == 0 {
        // subnormal: count from the highest set bit of the fraction
        let frac = x.to_bits() & ((1u64 << 52) - 1);
        -1022 - (frac.leading_zeros() as i32 - 12)
    } else {
        bits as i32 - 1022
    }
}

/// An exact product: z + zz == x * y.
fn dl_mul(x: f64, y: f64) -> (f64, f64) {
    let z = x * y;
    (z, x.mul_add(y, -z))
}

/// An exact sum, for |a| >= |b|: x + y == a + b.
fn dl_fast_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    (x, (a - x) + b)
}

/// NumPy's sum of a float64 array (`np.add.reduce`): pairwise, in blocks of eight, so a long sum can differ in the last
/// bit from adding the values one by one.
pub fn numpy_sum(values: &[f64]) -> f64 {
    0.0 + pairwise(values)
}

/// NumPy's mean of a float64 array: its sum, divided by the count.
pub fn numpy_mean(values: &[f64]) -> f64 {
    numpy_sum(values) / values.len() as f64
}

/// NumPy's `percentile(a, q)` with its default "linear" method: the value at (n - 1) * q / 100 in sorted order,
/// interpolated from both sides the way `_lerp` does it. NaN for no values.
pub fn numpy_percentile(values: &[f64], q: f64) -> f64 {
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let virtual_index = (n - 1) as f64 * (q / 100.0);
    if virtual_index >= (n - 1) as f64 {
        return sorted[n - 1];
    }
    if virtual_index < 0.0 {
        return sorted[0];
    }
    let prev = virtual_index.floor() as usize;
    lerp(sorted[prev], sorted[prev + 1], virtual_index - prev as f64)
}

/// NumPy's `_lerp`: from the near end, so the result never leaves [a, b].
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    let diff = b - a;
    if t >= 0.5 { b - diff * (1.0 - t) } else { a + diff * t }
}

/// NumPy's `median`: the middle value, or the mean of the two middle values. NaN for no values.
pub fn numpy_median(values: &[f64]) -> f64 {
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    if n % 2 == 1 { sorted[n / 2] } else { numpy_mean(&sorted[n / 2 - 1..=n / 2]) }
}

/// NumPy's `pairwise_sum` (numpy/_core/src/umath/loops_utils.h.src).
fn pairwise(a: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    let n = a.len();
    if n < 8 {
        let mut res = 0.0;
        for v in a {
            res += v;
        }
        res
    } else if n <= BLOCK {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - n % 8 {
            for (j, acc) in r.iter_mut().enumerate() {
                *acc += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_as_python_does() {
        assert_eq!(round(23.00034999, 4), 23.0003);
        assert_eq!(round(0.125, 2), 0.12);
        assert_eq!(round(-4.86225, 4), -4.8623); // stored as -4.862250000000000405: past the tie
        assert_eq!(round(2.5, 0), 2.0);
    }

    #[test]
    fn sums_in_numpys_order() {
        let a: Vec<f64> = (0..300).map(|i| 0.1 * i as f64 + 1e-9 * (i * i) as f64).collect();
        let by_one: f64 = a.iter().sum();
        assert!((numpy_sum(&a) - by_one).abs() < 1e-9);
        assert_eq!(numpy_sum(&[1.0, 2.0, 3.0]), 6.0);
        assert_eq!(numpy_mean(&[1.0, 2.0]), 1.5);
    }
}

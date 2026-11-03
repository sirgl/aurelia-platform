//! Deterministic real powers for the Classic cost formulas
//! (`int(pow(x, e))`: border growth 1.1, policies 2.01).
//!
//! `f64::powf` calls the platform's libm, whose last bits may differ
//! between platforms; a replay must not (platform/project.md §23). `pow` here uses
//! only IEEE-754 addition, multiplication and division, which are exact
//! and identical everywhere, with series accurate to ~1e-15 relative. The
//! tests compare it with `powf` on every value the game truncates.

/// ln 2, correctly rounded.
const LN_2: f64 = std::f64::consts::LN_2;

/// `x^e` for `x >= 0` (0 for `x <= 0`; `x^0 = 1`).
pub fn pow(x: f64, e: f64) -> f64 {
    if x <= 0.0 {
        return if e == 0.0 { 1.0 } else { 0.0 };
    }
    exp(e * ln(x))
}

/// `int(pow(x, e))` of the C++ formulas (truncation toward zero).
pub fn pow_trunc(x: u64, e: f64) -> u64 {
    pow(x as f64, e) as u64
}

/// Natural logarithm for `x > 0`: `x = m · 2^k` with `m` in [1, 2), then
/// `ln m = 2 atanh((m - 1) / (m + 1))`.
fn ln(x: f64) -> f64 {
    let (m, k) = split_exponent(x);
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut sum = 0.0;
    let mut n = 1.0;
    while n < 80.0 {
        sum += term / n;
        term *= z2;
        n += 2.0;
    }
    2.0 * sum + k as f64 * LN_2
}

/// `e^y`: `y = n ln2 + r`, |r| <= ln2/2, Taylor series of `e^r`, then the
/// exact scaling by `2^n`.
fn exp(y: f64) -> f64 {
    let n = (y / LN_2).round();
    let r = y - n * LN_2;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut i = 1.0;
    while i < 30.0 {
        term = term * r / i;
        sum += term;
        i += 1.0;
    }
    scale_by_power_of_two(sum, n as i32)
}

/// (m, k) with `x = m · 2^k`, `m` in [1, 2) (normal positive `x`).
fn split_exponent(x: f64) -> (f64, i32) {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let mantissa = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | (1023u64 << 52));
    (mantissa, exponent)
}

/// `v · 2^n` by repeated exact doubling or halving.
fn scale_by_power_of_two(mut v: f64, n: i32) -> f64 {
    let step = if n >= 0 { 2.0 } else { 0.5 };
    for _ in 0..n.unsigned_abs() {
        v *= step;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow_matches_powf_where_the_game_truncates() {
        // The exponents are single-precision floats, as in the Classic rules.
        let border = f64::from(1.1f32);
        let tradition = f64::from(1.1f32 * 75.0 / 100.0);
        let policy = f64::from(2.01f32);
        // Border growth: (10·level)^1.1 for every level a city can reach,
        // and with Tradition (exponent −25 %).
        for level in 0..=200u64 {
            let x = level * 10;
            assert_eq!(
                pow_trunc(x, border),
                (x as f64).powf(border) as u64,
                "x={x}"
            );
            assert_eq!(
                pow_trunc(x, tradition),
                (x as f64).powf(tradition) as u64,
                "x={x}"
            );
        }
        // Policies: (3·n)^2.01 for every policy count.
        for n in 0..=200u64 {
            let x = n * 3;
            assert_eq!(
                pow_trunc(x, policy),
                (x as f64).powf(policy) as u64,
                "x={x}"
            );
        }
    }

    #[test]
    fn pow_is_accurate() {
        for (x, e) in [
            (2.0, 0.5),
            (10.0, 1.1),
            (333.0, 2.01),
            (7.0, 3.0),
            (1.0, 5.0),
        ] {
            let exact: f64 = f64::powf(x, e);
            assert!((pow(x, e) - exact).abs() <= exact * 1e-13, "{x}^{e}");
        }
        assert_eq!(pow(0.0, 1.1), 0.0);
        assert_eq!(pow(5.0, 0.0), 1.0);
    }
}

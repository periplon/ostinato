//! CUE-compatible arbitrary-precision decimal arithmetic.
//!
//! CUE evaluates numbers with a 34 significant-digit decimal context
//! ([`cuelang.org/go/internal.BaseContext`] wraps `apd.BaseContext` with a
//! precision of 34).  Literals keep every digit they were written with, but
//! the result of an arithmetic operation is rounded to 34 significant digits
//! using round-half-even.  Division uses `apd.Context.Quo` and then reduces
//! trailing zeros while keeping a fractional `.0` for results that became
//! integral.
//!
//! The helpers in this module reproduce the observable behaviour of the CUE
//! v0.17.1 oracle: arithmetic results, `math` builtins and the decimal string
//! form used by JSON export and string interpolation.

use std::cmp::Ordering;
use std::num::NonZeroU64;

use bigdecimal::{BigDecimal, Context, RoundingMode};
use num_bigint::{BigInt, Sign, ToBigInt};

/// CUE's decimal context precision.
pub const CUE_PRECISION: u64 = 34;

/// Extra working precision used for transcendental functions.  CUE's `apd`
/// implementations compute with a few guard digits and round once at the end;
/// computing with 60 digits and rounding to 34 produces the same result for
/// every probe in the conformance corpus.
const WORK_PRECISION: u64 = 60;

fn nonzero(precision: u64) -> NonZeroU64 {
    NonZeroU64::new(precision).unwrap_or(NonZeroU64::MIN)
}

/// Round a decimal to `precision` significant digits (round-half-even).
#[must_use]
pub fn round_significant(value: &BigDecimal, precision: u64) -> BigDecimal {
    let (coefficient, scale) = value.as_bigint_and_exponent();
    let negative = coefficient.sign() == Sign::Minus;
    let coefficient = absolute(&coefficient);
    let digit_text = coefficient.to_string();
    let digits = u64::try_from(digit_text.len()).unwrap_or(u64::MAX);
    if digits <= precision {
        return value.clone();
    }
    let drop = digits - precision;
    let divisor = pow10(drop);
    let quotient = &coefficient / &divisor;
    let remainder = &coefficient % &divisor;
    // CUE's `apd.BaseContext` uses the zero-value rounder, `RoundHalfUp`:
    // ties are rounded away from zero.
    let mut rounded = if &remainder * BigInt::from(2) >= divisor {
        quotient + BigInt::from(1)
    } else {
        quotient
    };
    let mut new_scale = scale.saturating_sub(i64::try_from(drop).unwrap_or(i64::MAX));
    if u64::try_from(rounded.to_string().len()).unwrap_or(u64::MAX) > precision {
        rounded /= BigInt::from(10);
        new_scale = new_scale.saturating_sub(1);
    }
    if negative {
        rounded = -rounded;
    }
    BigDecimal::new(rounded, new_scale)
}

/// Round a decimal to CUE's 34-digit context.
#[must_use]
pub fn round_cue(value: &BigDecimal) -> BigDecimal {
    round_significant(value, CUE_PRECISION)
}

fn pow10(exponent: u64) -> BigInt {
    BigInt::from(10u8).pow(u32::try_from(exponent).unwrap_or(u32::MAX))
}

fn digit_count(value: &BigInt) -> i64 {
    let text = value.to_string();
    let digits = text.strip_prefix('-').unwrap_or(&text);
    i64::try_from(digits.len()).unwrap_or(i64::MAX)
}

/// Format a decimal exactly like `apd.Decimal.String()` (the GDA
/// to-scientific-string conversion), which CUE uses for JSON numbers and
/// string interpolation.
///
/// The rule is: use plain notation when the decimal exponent is non-positive
/// and the adjusted exponent is at least -6; otherwise use scientific
/// notation with an uppercase `E` and an explicit exponent sign.
#[must_use]
pub fn cue_decimal_string(value: &BigDecimal) -> String {
    let (coefficient, scale) = value.as_bigint_and_exponent();
    let negative = coefficient.sign() == Sign::Minus;
    let raw = coefficient.to_string();
    let digits = raw.strip_prefix('-').unwrap_or(&raw);
    let exponent = -scale;
    let digit_len = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let adjusted = exponent.saturating_add(digit_len).saturating_sub(1);

    if exponent <= 0 && adjusted >= -6 {
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        if exponent < 0 {
            let left = exponent.saturating_neg().saturating_sub(digit_len);
            if left >= 0 {
                out.push_str("0.");
                out.push_str(&"0".repeat(usize::try_from(left).unwrap_or(0)));
                out.push_str(digits);
            } else {
                let offset = usize::try_from(-left).unwrap_or(0);
                let offset = offset.min(digits.len());
                out.push_str(&digits[..offset]);
                out.push('.');
                out.push_str(&digits[offset..]);
            }
        } else {
            out.push_str(digits);
            out.push_str(&"0".repeat(usize::try_from(exponent).unwrap_or(0)));
        }
        return out;
    }

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    }
    out.push('E');
    if adjusted < 0 {
        out.push('-');
    } else {
        out.push('+');
    }
    out.push_str(&adjusted.abs().to_string());
    out
}

/// Convert a `f64` into the decimal representation CUE produces for a
/// `float64` builtin result (`apd.Decimal.SetFloat64` uses Go's
/// `strconv.FormatFloat(f, 'E', -1, 64)`).
#[must_use]
pub fn decimal_from_f64(value: f64) -> Option<BigDecimal> {
    if !value.is_finite() {
        return None;
    }
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e')?;
    let exponent: i64 = exponent.parse().ok()?;
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa),
    };
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let digit_len = i64::try_from(digits.len()).ok()?;
    let scale = digit_len.saturating_sub(1).saturating_sub(exponent);
    let mut coefficient: BigInt = digits.parse().ok()?;
    if negative && coefficient != BigInt::from(0) {
        coefficient = -coefficient;
    }
    Some(BigDecimal::new(coefficient, scale))
}

/// Exact addition, rounded to CUE's context.
#[must_use]
pub fn add(left: &BigDecimal, right: &BigDecimal) -> BigDecimal {
    let result = left + right;
    if result == 0 {
        let scale = left
            .as_bigint_and_exponent()
            .1
            .max(right.as_bigint_and_exponent().1);
        return BigDecimal::new(BigInt::from(0), scale);
    }
    round_cue(&result)
}

/// Exact subtraction, rounded to CUE's context.
#[must_use]
pub fn sub(left: &BigDecimal, right: &BigDecimal) -> BigDecimal {
    let result = left - right;
    if result == 0 {
        let scale = left
            .as_bigint_and_exponent()
            .1
            .max(right.as_bigint_and_exponent().1);
        return BigDecimal::new(BigInt::from(0), scale);
    }
    round_cue(&result)
}

/// Exact multiplication, rounded to CUE's context.
#[must_use]
pub fn mul(left: &BigDecimal, right: &BigDecimal) -> BigDecimal {
    // Do not use `BigDecimal`'s multiplication operator: it shortcuts when
    // either operand is numerically one and normalizes the other operand,
    // which would drop CUE's scale (`1.00 * 1.00` must stay `1.0000`).
    let (left_coefficient, left_scale) = left.as_bigint_and_exponent();
    let (right_coefficient, right_scale) = right.as_bigint_and_exponent();
    let scale = left_scale.saturating_add(right_scale);
    let coefficient = left_coefficient * right_coefficient;
    let result = BigDecimal::new(coefficient, scale);
    if result == 0 {
        return BigDecimal::new(BigInt::from(0), scale);
    }
    round_cue(&result)
}

/// Reproduce CUE's `reduceKeepingFloats` wrapper around `apd.Context.Quo`:
/// reduce trailing zeros but keep a single fractional zero for results that
/// became integral after reduction.
#[must_use]
pub fn reduce_keeping_floats(value: &BigDecimal) -> BigDecimal {
    let (_, old_scale) = value.as_bigint_and_exponent();
    let reduced = value.normalized();
    let (coefficient, scale) = reduced.as_bigint_and_exponent();
    if old_scale > 0 && scale <= 0 && coefficient != BigInt::from(0) {
        BigDecimal::new(coefficient * 10, scale + 1)
    } else if old_scale > 0 && scale <= 0 {
        BigDecimal::new(BigInt::from(0), 1)
    } else {
        reduced
    }
}

/// Division using CUE's 34-digit decimal expansion.
///
/// Returns `None` for division by zero.  The algorithm mirrors
/// `apd.Context.Quo` (the quotient is computed with exactly 34 significant
/// digits and a final remainder-based half-even rounding), then applies the
/// same trailing-zero reduction CUE performs.
#[must_use]
pub fn divide(left: &BigDecimal, right: &BigDecimal) -> Option<BigDecimal> {
    divide_with_precision(left, right, CUE_PRECISION).map(|value| reduce_keeping_floats(&value))
}

/// Same as [`divide`] but with an explicit significant-digit precision.
fn divide_with_precision(
    left: &BigDecimal,
    right: &BigDecimal,
    precision: u64,
) -> Option<BigDecimal> {
    let (left_coefficient, left_scale) = left.as_bigint_and_exponent();
    let (right_coefficient, right_scale) = right.as_bigint_and_exponent();
    if right_coefficient == BigInt::from(0) {
        return None;
    }
    let negative =
        (left_coefficient.sign() == Sign::Minus) != (right_coefficient.sign() == Sign::Minus);
    // `apd` exponents are the negated scales.
    let shift = right_scale - left_scale;

    if left_coefficient == BigInt::from(0) {
        let value = BigDecimal::new(BigInt::from(0), -shift);
        return Some(value);
    }

    let mut dividend = absolute(&left_coefficient);
    let mut divisor = absolute(&right_coefficient);
    let digit_difference = digit_count(&dividend) - digit_count(&divisor);
    let mut adjusted_coefficients = -digit_difference;
    if digit_difference < 0 {
        dividend *= pow10(u64::try_from(-digit_difference).unwrap_or(u64::MAX));
    } else if digit_difference > 0 {
        divisor *= pow10(u64::try_from(digit_difference).unwrap_or(u64::MAX));
    }
    if dividend < divisor {
        dividend *= BigInt::from(10);
        adjusted_coefficients += 1;
    }

    let precision = i64::try_from(precision).unwrap_or(i64::MAX);
    let adjustment = precision.saturating_sub(1);
    dividend *= pow10(u64::try_from(adjustment).unwrap_or(u64::MAX));

    let mut quotient = &dividend / &divisor;
    let remainder = &dividend % &divisor;
    if remainder != BigInt::from(0) {
        let twice = &remainder * BigInt::from(2);
        let ordering = twice.cmp(&divisor);
        let round_up = ordering != Ordering::Less;
        if round_up {
            quotient += BigInt::from(1);
        }
    }

    let exponent = shift
        .saturating_sub(adjusted_coefficients)
        .saturating_sub(adjustment);
    let coefficient = if negative { -quotient } else { quotient };
    Some(BigDecimal::new(coefficient, -exponent))
}

/// Negate without rounding (unary minus is exact in CUE).
#[must_use]
pub fn negate(value: &BigDecimal) -> BigDecimal {
    -value.clone()
}

/// CUE's decimal string for a number literal is the same GDA form.
#[must_use]
pub fn parse_number(text: &str) -> Option<BigDecimal> {
    text.parse().ok()
}

/// Round to an exact integer, returning the expanded decimal with exponent 0.
/// This mirrors `apd.Context.RoundToIntegralExact` followed by `fromGoValue`'s
/// integral-kind detection.
#[must_use]
pub fn expand_integral(value: &BigDecimal) -> Option<BigDecimal> {
    if !value.is_integer() {
        return None;
    }
    Some(value.with_scale(0))
}

fn working_round(value: &BigDecimal) -> BigDecimal {
    round_significant(value, WORK_PRECISION)
}

fn absolute(value: &BigInt) -> BigInt {
    if value < &BigInt::from(0) {
        -value.clone()
    } else {
        value.clone()
    }
}

fn absolute_decimal(value: &BigDecimal) -> BigDecimal {
    if value < 0 {
        -value.clone()
    } else {
        value.clone()
    }
}

fn decimal_ln2() -> BigDecimal {
    // ln(2) = 2 * atanh(1/3).
    let one = BigDecimal::from(1);
    let three = BigDecimal::from(3);
    let z = working_round(&(one.clone() / three));
    atanh_series_times_two(&z)
}

fn atanh_series_times_two(z: &BigDecimal) -> BigDecimal {
    let z_squared = working_round(&(z * z));
    let mut term = z.clone();
    let mut sum = z.clone();
    let mut denominator = 3i64;
    for _ in 0..200 {
        term = working_round(&(&term * &z_squared));
        let contribution = working_round(&(&term / BigDecimal::from(denominator)));
        let next = working_round(&(&sum + &contribution));
        let done = next == sum;
        sum = next;
        if done {
            break;
        }
        denominator += 2;
    }
    working_round(&(sum * BigDecimal::from(2)))
}

/// Natural logarithm with enough guard digits to round correctly to CUE's
/// 34-digit context.
#[must_use]
pub fn ln(value: &BigDecimal) -> Option<BigDecimal> {
    if value <= 0 {
        return None;
    }
    if value == &BigDecimal::from(1) {
        return Some(BigDecimal::from(0));
    }
    let two = BigDecimal::from(2);
    let mut mantissa = value.clone();
    let mut exponent = 0i64;
    while mantissa >= two {
        mantissa = working_round(&(mantissa / &two));
        exponent += 1;
    }
    let half = BigDecimal::from(1) / BigDecimal::from(2);
    while mantissa < half {
        mantissa = working_round(&(mantissa * &two));
        exponent -= 1;
    }
    let one = BigDecimal::from(1);
    let z = working_round(&((&mantissa - &one) / (&mantissa + &one)));
    let mut result = atanh_series_times_two(&z);
    if exponent != 0 {
        let ln2 = decimal_ln2();
        result = working_round(&(&result + &(ln2 * BigDecimal::from(exponent))));
    }
    Some(result)
}

/// `e^x` with guard digits.
#[must_use]
pub fn exp(value: &BigDecimal) -> Option<BigDecimal> {
    if value == 0 {
        return Some(BigDecimal::from(1));
    }
    let context = Context::new(nonzero(WORK_PRECISION), RoundingMode::HalfEven);
    let result = value.exp_with_context(&context);
    Some(working_round(&result))
}

/// `x^y` matching `apd.Context.Pow` for CUE's context.
#[must_use]
pub fn pow(base: &BigDecimal, exponent: &BigDecimal) -> Option<BigDecimal> {
    if base == 0 && exponent == 0 {
        return None;
    }
    if exponent == 0 {
        return Some(BigDecimal::from(1));
    }
    if base == 0 {
        if exponent > 0 {
            return Some(BigDecimal::from(0));
        }
        return None;
    }

    let integer = exponent.with_scale_round(0, RoundingMode::Down);
    let fraction = working_round(&(exponent - &integer));
    let negative_base = base < 0;
    if negative_base && fraction != 0 {
        return None;
    }

    let digits = i64::try_from(absolute(&base.as_bigint_and_exponent().0).to_string().len())
        .unwrap_or(i64::MAX);
    let mut precision = CUE_PRECISION.max(u64::try_from(digits).unwrap_or(u64::MAX));
    precision = precision.saturating_add(10);

    let integer_coefficient: Option<BigInt> = integer.to_bigint();
    let mut integer_part = match integer_coefficient {
        Some(value) => integer_power(&absolute_decimal(base), &value, precision)?,
        None => BigDecimal::from(1),
    };

    if fraction != 0 {
        let logarithm = ln(&absolute_decimal(base))?;
        let scaled = round_significant(&(logarithm * &fraction), precision);
        let factor = exp(&scaled)?;
        integer_part = round_significant(&(integer_part * factor), precision);
    }

    let mut result = round_cue(&integer_part);
    if negative_base {
        // Only integer exponents can have a negative base, and odd integer
        // exponents produce a negative result.
        if let Some(value) = exponent.to_bigint()
            && value.bit(0)
        {
            result = -result;
        }
    }
    Some(result)
}

fn integer_power(base: &BigDecimal, exponent: &BigInt, precision: u64) -> Option<BigDecimal> {
    let negative = exponent.sign() == Sign::Minus;
    let mut power = absolute(exponent);
    let mut factor = base.clone();
    let mut result = BigDecimal::from(1);
    while power > BigInt::from(0) {
        if power.bit(0) {
            result = round_significant(&(result * &factor), precision);
        }
        power >>= 1;
        if power > BigInt::from(0) {
            factor = round_significant(&(factor.clone() * &factor), precision);
        }
    }
    if negative {
        result = divide_with_precision(&BigDecimal::from(1), &result, precision)?;
    }
    Some(result)
}

/// Cube root using the decimal logarithm/exponential pair.
#[must_use]
pub fn cbrt(value: &BigDecimal) -> Option<BigDecimal> {
    if value == 0 {
        return Some(BigDecimal::from(0));
    }
    let negative = value < 0;
    let magnitude = absolute_decimal(value);
    let logarithm = ln(&magnitude)?;
    let third = working_round(&(logarithm / BigDecimal::from(3)));
    let root = exp(&third)?;
    Some(if negative { -root } else { root })
}

/// `log10` computed from the natural logarithm with guard digits.
#[must_use]
pub fn log10(value: &BigDecimal) -> Option<BigDecimal> {
    let logarithm = ln(value)?;
    let ln10 = ln(&BigDecimal::from(10))?;
    Some(working_round(&(logarithm / ln10)))
}

/// CUE's `math.Log2`: both logarithms are rounded to the 34-digit context
/// before the final reduced division.
#[must_use]
pub fn log2(value: &BigDecimal) -> Option<BigDecimal> {
    let numerator = round_cue(&ln(value)?);
    let denominator = round_cue(&ln(&BigDecimal::from(2))?);
    divide(&numerator, &denominator)
}

/// CUE's `math.Exp2`: `Pow(2, x)`.
#[must_use]
pub fn exp2(value: &BigDecimal) -> Option<BigDecimal> {
    pow(&BigDecimal::from(2), value)
}

#[cfg(test)]
#[allow(clippy::approx_constant, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dec(text: &str) -> BigDecimal {
        text.parse().expect("decimal")
    }

    #[test]
    fn formats_like_apd() {
        assert_eq!(cue_decimal_string(&dec("1E+400")), "1E+400");
        assert_eq!(cue_decimal_string(&dec("1.2300")), "1.2300");
        assert_eq!(cue_decimal_string(&dec("0.000")), "0.000");
        assert_eq!(cue_decimal_string(&dec("1e2")), "1E+2");
        assert_eq!(
            cue_decimal_string(&dec("0.00000000000000000000000000000000000001")),
            "1E-38"
        );
        assert_eq!(cue_decimal_string(&dec("0.000123")), "0.000123");
        assert_eq!(cue_decimal_string(&dec("1234")), "1234");
        assert_eq!(cue_decimal_string(&dec("-1.50")), "-1.50");
    }

    #[test]
    fn rounds_integer_arithmetic_to_34_digits() {
        let sum = add(&dec("12345678901234567890123456789012345"), &dec("1"));
        assert_eq!(
            cue_decimal_string(&sum),
            "1.234567890123456789012345678901235E+34"
        );
        let product = mul(&dec("1234567890123456789012345678901234567890"), &dec("2"));
        assert_eq!(
            cue_decimal_string(&product),
            "2.469135780246913578024691357802469E+39"
        );
    }

    #[test]
    fn rounds_ties_away_from_zero() {
        assert_eq!(
            cue_decimal_string(&add(&dec("99999999999999999999999999999999985"), &dec("0"))),
            "9.999999999999999999999999999999999E+34"
        );
        assert_eq!(
            cue_decimal_string(&add(&dec("12345678901234567890123456789012345"), &dec("0"))),
            "1.234567890123456789012345678901235E+34"
        );
        assert_eq!(
            cue_decimal_string(&sub(
                &dec(
                    "-1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                ),
                &dec("1")
            )),
            "-1.000000000000000000000000000000000E+99"
        );
    }

    #[test]
    fn divisions_with_34_digit_expansion() {
        assert_eq!(
            cue_decimal_string(&divide(&dec("1"), &dec("3")).expect("div")),
            "0.3333333333333333333333333333333333"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("2"), &dec("3")).expect("div")),
            "0.6666666666666666666666666666666667"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("100"), &dec("3")).expect("div")),
            "33.33333333333333333333333333333333"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("10"), &dec("2")).expect("div")),
            "5.0"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("4"), &dec("2")).expect("div")),
            "2.0"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("0"), &dec("3")).expect("div")),
            "0"
        );
        assert_eq!(
            cue_decimal_string(&divide(&dec("0.0"), &dec("2")).expect("div")),
            "0.0"
        );
        assert!(divide(&dec("1"), &dec("0")).is_none());
    }

    #[test]
    fn matches_cue_transcendentals() {
        assert_eq!(
            cue_decimal_string(&round_cue(&exp(&dec("1")).expect("exp"))),
            "2.718281828459045235360287471352662"
        );
        assert_eq!(
            cue_decimal_string(&round_cue(&ln(&dec("10")).expect("ln"))),
            "2.302585092994045684017991454684364"
        );
        assert_eq!(
            cue_decimal_string(&round_cue(&pow(&dec("2"), &dec("0.5")).expect("pow"))),
            "1.414213562373095048801688724209698"
        );
        assert_eq!(
            cue_decimal_string(&round_cue(&cbrt(&dec("2")).expect("cbrt"))),
            "1.259921049894873164767210607278228"
        );
    }

    #[test]
    fn converts_float64_like_apd() {
        assert_eq!(
            cue_decimal_string(&decimal_from_f64(10.0).expect("f")),
            "1E+1"
        );
        assert_eq!(
            cue_decimal_string(&decimal_from_f64(1.4142135623730951).expect("f")),
            "1.4142135623730951"
        );
        assert_eq!(
            cue_decimal_string(&decimal_from_f64(0.0000001).expect("f")),
            "1E-7"
        );
    }
}

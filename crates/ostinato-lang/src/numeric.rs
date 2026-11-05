//! CUE-compatible arbitrary-precision decimal arithmetic.
//!
//! CUE evaluates numbers with a 34 significant-digit decimal context
//! ([`cuelang.org/go/internal.BaseContext`] wraps `apd.BaseContext` with a
//! precision of 34).  Literals keep every digit they were written with, but
//! the result of an arithmetic operation is rounded to 34 significant digits
//! using round-half-up (the zero-value `apd.Rounder` used by CUE's
//! `BaseContext`).  The apd transcendental loops use their own internal
//! round-half-even contexts and only the final result is rounded into CUE's
//! context.  Division uses `apd.Context.Quo` and then reduces trailing zeros
//! while keeping a fractional `.0` for results that became integral.
//!
//! The helpers in this module reproduce the observable behaviour of the CUE
//! v0.17.1 oracle: arithmetic results, `math` builtins and the decimal string
//! form used by JSON export and string interpolation.

use bigdecimal::{BigDecimal, RoundingMode, Zero};
use num_bigint::{BigInt, Sign, ToBigInt};
use std::cmp::Ordering;

/// CUE's decimal context precision.
pub const CUE_PRECISION: u64 = 34;

/// Guard precision used by the `Pow` path.  `Ln`, `Exp`, `Log10`, `Log2` and
/// `Cbrt` port `apd`'s own iteration/rounding rules; this constant is only for
/// the shared power helper and probe-tested values.
const WORK_PRECISION: u64 = 60;

/// Round a decimal to `precision` significant digits (round-half-up, CUE's
/// arithmetic context).
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

/// A decimal value with CUE's signed-zero information.
///
/// `bigdecimal::BigDecimal` has no signed zero, but CUE's `apd.Decimal`
/// preserves the sign of zero produced by arithmetic (`0.0 * -1` exports
/// `-0.0`).  The sign is kept alongside the (always non-negative) magnitude so
/// that equality, ordering and arithmetic continue to treat it as zero.
#[derive(Debug, Clone)]
pub struct Decimal {
    value: BigDecimal,
    negative_zero: bool,
    /// True when the value came directly from a numeric literal (including a
    /// negated literal). Arithmetic and builtin results are computed.
    literal: bool,
}

impl Decimal {
    /// Wrap a non-negative-zero decimal.
    #[must_use]
    pub fn new(value: BigDecimal) -> Self {
        Self {
            value,
            negative_zero: false,
            literal: true,
        }
    }

    /// Wrap an arithmetic or builtin result.
    #[must_use]
    pub fn computed(value: BigDecimal) -> Self {
        Self {
            value,
            negative_zero: false,
            literal: false,
        }
    }

    /// Build a negative zero with the given decimal scale (for example scale
    /// 1 renders `-0.0` and scale 0 renders `-0`).
    #[must_use]
    pub fn negative_zero(scale: i64) -> Self {
        Self {
            value: BigDecimal::new(BigInt::from(0), scale),
            negative_zero: true,
            literal: false,
        }
    }

    /// The underlying magnitude.
    #[must_use]
    pub fn value(&self) -> &BigDecimal {
        &self.value
    }

    /// Consume the wrapper and return the magnitude.
    #[must_use]
    pub fn into_inner(self) -> BigDecimal {
        self.value
    }

    /// Whether this is CUE's signed zero.
    #[must_use]
    pub fn is_negative_zero(&self) -> bool {
        self.negative_zero
    }

    /// Whether the value came from a syntactic numeric literal.
    #[must_use]
    pub fn is_literal(&self) -> bool {
        self.literal
    }

    /// Mark a value as having been read through a reference. CUE gives a
    /// reference lower representation priority than a syntactic literal or
    /// expression in `&` (`(1.0 + 0) & a` keeps `1.0`, not `a`'s `1.00`).
    #[must_use]
    pub fn as_reference(mut self) -> Self {
        self.literal = false;
        self
    }

    /// Whether the value is negative for sign-propagation purposes (either a
    /// negative magnitude or negative zero).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.negative_zero || self.value.sign() == Sign::Minus
    }

    /// The decimal scale, used when formatting zero results.
    #[must_use]
    pub fn scale(&self) -> i64 {
        self.value.as_bigint_and_exponent().1
    }
}

impl std::ops::Deref for Decimal {
    type Target = BigDecimal;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        // CUE compares `-0.0` and `0.0` as equal; the sign only affects the
        // rendered representation.
        self.value == other.value
    }
}

impl Eq for Decimal {}

impl PartialEq<BigDecimal> for Decimal {
    fn eq(&self, other: &BigDecimal) -> bool {
        &self.value == other
    }
}

impl PartialEq<Decimal> for BigDecimal {
    fn eq(&self, other: &Decimal) -> bool {
        self == &other.value
    }
}

/// Render a decimal exactly like `apd.Decimal.String()`, including a leading
/// `-` for signed zero.
#[must_use]
pub fn decimal_display(value: &Decimal) -> String {
    let text = cue_decimal_string(&value.value);
    if value.negative_zero {
        format!("-{text}")
    } else {
        text
    }
}

/// Convert a `f64` into the decimal representation CUE produces for a
/// `float64` builtin result (`apd.Decimal.SetFloat64` uses Go's
/// `strconv.FormatFloat(f, 'E', -1, 64)`).
#[must_use]
pub fn decimal_from_f64(value: f64) -> Option<Decimal> {
    if !value.is_finite() {
        return None;
    }
    // Go's `apd.Decimal.SetFloat64` uses `strconv.FormatFloat(f, 'E', -1, 64)`.
    // `ryu` produces the same shortest round-trip digits; normalizing removes
    // the `.0` spelling for integral values so the decimal exponent matches
    // Go's `E` form.
    let mut buffer = ryu::Buffer::new();
    let text = buffer.format_finite(value);
    let value: BigDecimal = text.parse().ok()?;
    let value = value.normalized();
    let negative_zero = text.starts_with('-');
    if negative_zero && value.is_zero() {
        Some(Decimal::negative_zero(0))
    } else {
        Some(Decimal::computed(value))
    }
}

/// Errors raised by CUE-compatible arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericError {
    DivisionByZero,
    ExponentOutOfRange,
    Overflow,
    Underflow,
    InvalidOperation,
}

impl NumericError {
    /// Message used by the evaluator.  CUE's own errors are
    /// `failed arithmetic: ...`/`error in call to math.X: ...` wrappers around
    /// these phrases.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::DivisionByZero => "division by zero",
            Self::ExponentOutOfRange => "exponent out of range",
            Self::Overflow => "overflow",
            Self::Underflow => "underflow, subnormal",
            Self::InvalidOperation => "invalid operation",
        }
    }
}

type NumericResult<T> = Result<T, NumericError>;

/// `apd.MaxExponent`.
pub const MAX_EXPONENT: i64 = 100_000;
/// `apd.MinExponent`.
pub const MIN_EXPONENT: i64 = -100_000;

fn apd_exponent(value: &BigDecimal) -> i64 {
    -value.as_bigint_and_exponent().1
}

fn num_digits(value: &BigDecimal) -> i64 {
    let (coefficient, _) = value.as_bigint_and_exponent();
    digit_count(&coefficient)
}

fn adjusted_exponent(value: &BigDecimal) -> i64 {
    let (coefficient, scale) = value.as_bigint_and_exponent();
    if coefficient == BigInt::from(0) {
        return 0;
    }
    -scale + digit_count(&coefficient) - 1
}

fn check_exponent(value: &BigDecimal) -> NumericResult<()> {
    let exponent = adjusted_exponent(value);
    if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
        Err(NumericError::ExponentOutOfRange)
    } else {
        Ok(())
    }
}

fn round_with_mode(value: &BigDecimal, precision: u64, mode: RoundingMode) -> BigDecimal {
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
    let twice = &remainder * BigInt::from(2);
    let round_up = match mode {
        RoundingMode::HalfEven => twice > divisor || (twice == divisor && quotient.bit(0)),
        RoundingMode::Up => remainder != BigInt::from(0),
        RoundingMode::Down => false,
        // CUE's default (`RoundHalfUp`, the zero-value `apd.Rounder`):
        // ties are rounded away from zero.
        _ => twice >= divisor,
    };
    let mut rounded = if round_up {
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

fn add_at(
    left: &BigDecimal,
    right: &BigDecimal,
    precision: u64,
    mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    let (left_coefficient, left_scale) = left.as_bigint_and_exponent();
    let (right_coefficient, right_scale) = right.as_bigint_and_exponent();
    let scale = left_scale.max(right_scale);
    if scale.abs_diff(left_scale.min(right_scale)) > u64::try_from(MAX_EXPONENT).unwrap_or(u64::MAX)
    {
        return Err(NumericError::ExponentOutOfRange);
    }
    // Align exactly like apd's `upscale`; `BigDecimal`'s own addition caps the
    // scale extension it will materialize, which truncates large exponents.
    let left_coefficient =
        left_coefficient * pow10(u64::try_from(scale - left_scale).unwrap_or(u64::MAX));
    let right_coefficient =
        right_coefficient * pow10(u64::try_from(scale - right_scale).unwrap_or(u64::MAX));
    let coefficient = left_coefficient + right_coefficient;
    let result = BigDecimal::new(coefficient, scale);
    if result == 0 {
        return Ok(result);
    }
    let rounded = round_with_mode(&result, precision, mode);
    check_exponent(&rounded)?;
    Ok(rounded)
}

fn mul_at(
    left: &BigDecimal,
    right: &BigDecimal,
    precision: u64,
    mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    // apd's Mul checks both operand exponents, then the adjusted exponent of
    // the exact product.
    for operand in [left, right] {
        let exponent = apd_exponent(operand);
        if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
            return Err(NumericError::ExponentOutOfRange);
        }
    }
    let (left_coefficient, left_scale) = left.as_bigint_and_exponent();
    let (right_coefficient, right_scale) = right.as_bigint_and_exponent();
    let scale = left_scale.saturating_add(right_scale);
    let coefficient = left_coefficient * right_coefficient;
    let result = BigDecimal::new(coefficient, scale);
    if result == 0 {
        return Ok(BigDecimal::new(BigInt::from(0), scale));
    }
    // apd's `round` re-checks the pre-rounding raw exponent against
    // Min/MaxExponent before dropping digits.
    if !(-MAX_EXPONENT..=MAX_EXPONENT).contains(&scale) {
        return Err(NumericError::ExponentOutOfRange);
    }
    let rounded = round_with_mode(&result, precision, mode);
    check_exponent(&rounded)?;
    Ok(rounded)
}

fn quo_at(
    left: &BigDecimal,
    right: &BigDecimal,
    precision: u64,
    mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    divide_with_precision_mode(left, right, precision, mode)
}

/// Exact addition, rounded to CUE's context.
pub fn add(left: &BigDecimal, right: &BigDecimal) -> NumericResult<BigDecimal> {
    add_at(left, right, CUE_PRECISION, RoundingMode::HalfUp)
}

/// Exact subtraction, rounded to CUE's context.
pub fn sub(left: &BigDecimal, right: &BigDecimal) -> NumericResult<BigDecimal> {
    add_at(
        left,
        &negate_raw(right),
        CUE_PRECISION,
        RoundingMode::HalfUp,
    )
}

/// Exact multiplication, rounded to CUE's context.
pub fn mul(left: &BigDecimal, right: &BigDecimal) -> NumericResult<BigDecimal> {
    mul_at(left, right, CUE_PRECISION, RoundingMode::HalfUp)
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
/// Returns [`NumericError::DivisionByZero`] for a zero divisor and
/// [`NumericError::ExponentOutOfRange`] when apd's exponent limits are
/// exceeded.
pub fn divide(left: &BigDecimal, right: &BigDecimal) -> NumericResult<BigDecimal> {
    let value = divide_with_precision_mode(left, right, CUE_PRECISION, RoundingMode::HalfUp)?;
    let reduced = reduce_keeping_floats(&value);
    check_exponent(&reduced)?;
    Ok(reduced)
}

fn divide_with_precision_mode(
    left: &BigDecimal,
    right: &BigDecimal,
    precision: u64,
    mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    let (left_coefficient, left_scale) = left.as_bigint_and_exponent();
    let (right_coefficient, right_scale) = right.as_bigint_and_exponent();
    if right_coefficient == BigInt::from(0) {
        return Err(NumericError::DivisionByZero);
    }
    let negative =
        (left_coefficient.sign() == Sign::Minus) != (right_coefficient.sign() == Sign::Minus);
    // `apd` exponents are the negated scales.
    let shift = right_scale - left_scale;

    if left_coefficient == BigInt::from(0) {
        let value = BigDecimal::new(BigInt::from(0), -shift);
        check_exponent(&value)?;
        return Ok(value);
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
        let round_up = match mode {
            RoundingMode::HalfEven => {
                ordering == Ordering::Greater || (ordering == Ordering::Equal && quotient.bit(0))
            }
            RoundingMode::Down => false,
            _ => ordering != Ordering::Less,
        };
        if round_up {
            quotient += BigInt::from(1);
        }
    }

    let exponent = shift
        .saturating_sub(adjusted_coefficients)
        .saturating_sub(adjustment);
    let coefficient = if negative { -quotient } else { quotient };
    let value = BigDecimal::new(coefficient, -exponent);
    check_exponent(&value)?;
    Ok(value)
}

fn negate_raw(value: &BigDecimal) -> BigDecimal {
    -value.clone()
}

/// Negate without rounding (unary minus is exact in CUE).
pub fn negate(value: &BigDecimal) -> NumericResult<BigDecimal> {
    let value = negate_raw(value);
    check_exponent(&value)?;
    Ok(value)
}

/// CUE's decimal string for a number literal is the same GDA form.
#[must_use]
pub fn parse_number(text: &str) -> Option<BigDecimal> {
    text.parse().ok()
}

/// Round to an exact integer, returning the expanded decimal with exponent 0.
/// This mirrors `apd.Context.RoundToIntegralExact` followed by `fromGoValue`'s
/// integral-kind detection.  Returns an exponent error instead of trying to
/// materialize an integer beyond apd's limits.
pub fn expand_integral(value: &BigDecimal) -> NumericResult<Option<BigDecimal>> {
    if !value.is_integer() {
        return Ok(None);
    }
    check_exponent(value)?;
    Ok(Some(value.with_scale(0)))
}

fn working_round(value: &BigDecimal, precision: u64) -> BigDecimal {
    round_with_mode(value, precision, RoundingMode::HalfEven)
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

fn f64_of(value: &BigDecimal) -> Option<f64> {
    cue_decimal_string(value).parse::<f64>().ok()
}

// Constants used by apd's `constWithPrecision` look-up.  `get(36)` selects the
// power-of-two bucket rounded to 64 digits with RoundHalfUp.
const LN10_DIGITS: &str = "2.3025850929940456840179914546843642076011014886287729760333279009675726096773524802359972050895982983419677840422862486334095254650828067566662873690987816894829072083255546808437998948262331985283935053089653777326288461633662222876982198867465436674744042432743651550489343149393914796194044002221051017141748003688084012647080685567743216228355220114804663715659121373450747856947683463616792101806445070648000277502684916746550586856935673420670581136429224554405758925724208241314695689016758940256776311356919292033376587141660230105703089634572075440370847469940168269282808481184289314848524948644871927809676271275775397027668605952496716674183485704422507197965004714951050492214776567636938662976979522110718264549734772662425709429322582798502585509785265383207606726317164309505995087807523710333101197857547331541421808427543863591778117054309827482385045648019095610299291824318237525357709750539565187697510374970888692180205189339507238539205144634197265287286965110862571492198849978748873771345686209167058498078280597511938544450099781311469159346662410718466923101075984383191912922307925037472986509290098803919417026544168163357275557031515961135648465461908970428197633658369837163289821744073660091621778505417792763677311450417821376601110107310423978325218948988175979217986663943195239368559164471182467532456309125287783309636042629821530408745609277607266413547875766162629265682987049579549139549180492090694385807900327630179415031178668620924085379498612649334793548717374516758095370882810674524401058924449764796860751202757241818749893959716431055188481952883307466993178146349300003212003277656541304726218839705967944579434683432183953044148448037013057536742621536755798147704580314136377932362915601281853364984669422614652064599420729171193706024449293580370077189810973625332245483669885055282859661928050984471751985036666808749704969822732202448233430971691111368135884186965493237149969419796878030088504089796185987565798948364452120436982164152929878117429733325886079159125109671875109292484750239305726654462762009230687915181358034777012955936462984123664970233551745861955647724618577173693684046765770478743197805738532718109338834963388130699455693993461010907456160333122479493604553618491233330637047517248712763791409243983318101647378233796922656376820717069358463945316169494117018419381194054164494661112747128197058177832938417422314099300229115023621921867233372683856882735333719251034129307056325444266114297653883018223840910261985828884335874559604530045483707890525784731662837019533922310475275649981192287427897137157132283196410034221242100821806795252766898581809561192083917607210809199234615169525990994737827806481280587927319938934534153201859697110214075422827962982370689417647406422257572124553925261793736524344405605953365915391603125244801493132345724538795243890368392364505078817313597112381453237015084134911223243909276817247496079557991513639828810582857405380006533716555530141963322419180876210182049194926514838926922937079";
const INVLN10_DIGITS: &str = "0.4342944819032518276511289189166050822943970058036665661144537831658646492088707747292249493384317483187061067447663037336416792871589639065692210646628122658521270865686703295933708696588266883311636077384905142844348666768646586085135561482123487653435434357317253835622281395603048646652366095539377356176323431916710991411597894962993512457934926357655469077671082419150479910989674900103277537653570270087328550951731440674697951899513594088040423931518868108402544654089797029863286828762624144013457043546132920600712605104028367125954846287707861998992326748439902348171535934551079475492552482577820679220140931468164467381030560475635720408883383209488996522717494541331791417640247407505788767860971099257547730046048656049515610057985741340272675201439247917970859047931285212493341197329877226463885350226083881626316463883553685501768460295286399391633510647555704050513182342988874882120643595023818902643317711537382203362634416478397146001858396093006317333986134035135741787144971453076492968331392399810608505734816169809280016199523523117237676561989228127013815804248715978344927215947562057179993483814031940166771520104787197582531617951490375597514246570736646439756863149325162498727994852637448791165959219701720662704559284657036462635675733575739369673994570909602526350957193468839951236811356428010958778313759442713049980643798750414472095974872674060160650105375287000491167867133309154761441005054775930890767885596533432190763128353570304854020979941614010807910607498871752495841461303867532086001324486392545573072842386175970677989354844570318359336523016027971626535726514428519866063768635338181954876389161343652374759465663921380736144503683797876824369028804493640496751871720614130731804417180216440993200651069696951247072666224570004229341407923361685302418860272411867806272570337552562870767696632173672454758133339263840130320038598899947332285703494195837691472090608812447825078736711573033931565625157907093245370450744326623349807143038059581776957944070042202545430531910888982754062263600601879152267477788232096025228766762416332296812464502577295040226623627536311798532153780883272326920785980990757434437367248710355853306546581653535157943990070326436222520010336980419843015524524173190520247212241110927324425302930200871037337504867498689117225672067268275246578790446735268575794059983346595878592624978725380185506389602375304294539963737367434680767515249986297676732404903363175488195323680087668648666069282082342536311304939972702858872849086258458687045569244548538607202497396631126372122497538854967981580284810494724140453341192674240839673061167234256843129624666246259542760677182858963306586513950932049023032806357536242804315480658368852257832901530787483141985929074121415344772165398214847619288406571345438798607895199435011532826457742311266817183284968697890904324421005272233475053141625981646457044538901148313760708445483457955728303866473638468537587172210685993933008378534367552699899185150879055911525282664";

fn ln10_const() -> BigDecimal {
    let value: BigDecimal = LN10_DIGITS.parse().unwrap_or_else(|_| BigDecimal::from(0));
    round_with_mode(&value, 64, RoundingMode::HalfUp)
}

fn inv_ln10_const() -> BigDecimal {
    let value: BigDecimal = INVLN10_DIGITS
        .parse()
        .unwrap_or_else(|_| BigDecimal::from(0));
    round_with_mode(&value, 64, RoundingMode::HalfUp)
}

/// Port of `apd.Context.Exp` at an explicit precision.  The algorithm is the
/// Hull-Abrham variable-precision exponential used by `apd`, including its
/// stage-1 overflow guard and stage-4 series.
fn apd_exp_at(
    value: &BigDecimal,
    precision: u64,
    _mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    if value == 0 {
        return Ok(BigDecimal::from(1));
    }
    let mut cp = precision;
    let absolute_value = absolute_decimal(value);
    if let Some(float) = f64_of(&absolute_value)
        && float.is_finite()
    {
        let needed = float / 23.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        if needed > cp as f64 && needed < 1000.0 {
            cp = needed.ceil() as u64;
        }
    }
    let limit = BigDecimal::from(cp.saturating_mul(23));
    if absolute_value > limit {
        return Err(if value < &BigDecimal::from(0) {
            NumericError::Underflow
        } else {
            NumericError::Overflow
        });
    }
    let tiny = BigDecimal::new(BigInt::from(9), i64::try_from(cp).unwrap_or(i64::MAX) + 1);
    if absolute_value <= tiny {
        return Ok(BigDecimal::from(1));
    }

    let mut t = apd_exponent(value).saturating_add(num_digits(value));
    if t < 0 {
        t = 0;
    }
    let power = BigDecimal::new(BigInt::from(1), -t);
    let remainder = quo_at(value, &power, cp, RoundingMode::HalfEven)?;
    let remainder_abs = absolute_decimal(&remainder);
    let iteration_precision = cp
        .saturating_add(u64::try_from(t).unwrap_or(u64::MAX))
        .saturating_add(2);

    let rf = f64_of(&remainder_abs).ok_or(NumericError::InvalidOperation)?;
    if rf == 0.0 {
        return Ok(BigDecimal::from(1));
    }
    #[allow(clippy::cast_precision_loss)]
    let pf = iteration_precision as f64;
    let log10 = crate::gomath::log(pf / rf) * (1.0 / std::f64::consts::LN_10);
    let iterations = ((1.435 * pf - 1.182) / log10).ceil();
    if !iterations.is_finite() || iterations > 1000.0 {
        return Err(NumericError::Overflow);
    }
    #[allow(clippy::cast_possible_truncation)]
    let n = iterations as i64;

    let mut sum = BigDecimal::from(1);
    let mut index = n.saturating_sub(1);
    while index > 0 {
        let term = quo_at(
            &remainder,
            &BigDecimal::from(index),
            iteration_precision,
            RoundingMode::HalfEven,
        )?;
        sum = mul_at(&term, &sum, iteration_precision, RoundingMode::HalfEven)?;
        sum = add_at(
            &sum,
            &BigDecimal::from(1),
            iteration_precision,
            RoundingMode::HalfEven,
        )?;
        index -= 1;
    }
    let power_integer = pow10(u64::try_from(t).unwrap_or(u64::MAX));
    let result = integer_power_at(
        &sum,
        &power_integer,
        iteration_precision,
        RoundingMode::HalfEven,
    )?;
    let rounded = round_with_mode(&result, precision, RoundingMode::HalfEven);
    check_exponent(&rounded)?;
    Ok(rounded)
}

/// Port of `apd.Context.Ln` at an explicit output precision/rounding.
fn apd_ln_at(
    value: &BigDecimal,
    final_precision: u64,
    final_mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    if value <= 0 {
        return Err(NumericError::InvalidOperation);
    }
    if value == &BigDecimal::from(1) {
        return Ok(BigDecimal::from(0));
    }
    let precision = final_precision.saturating_add(2);
    let mut z = value.clone();
    let mut result_adjust = BigDecimal::from(0);
    let mut tmp1 = add_at(&z, &BigDecimal::from(-1), precision, RoundingMode::HalfEven)?;
    let tenth = BigDecimal::new(BigInt::from(1), 1);
    let mut use_power_series = absolute_decimal(&tmp1) <= tenth;

    if !use_power_series {
        let exponent_delta = num_digits(&z).saturating_add(apd_exponent(&z));
        let (coefficient, scale) = z.as_bigint_and_exponent();
        z = BigDecimal::new(coefficient, scale.saturating_add(exponent_delta));
        result_adjust = mul_at(
            &BigDecimal::from(exponent_delta),
            &ln10_const(),
            precision,
            RoundingMode::HalfEven,
        )?;
        tmp1 = add_at(&z, &BigDecimal::from(-1), precision, RoundingMode::HalfEven)?;
        if absolute_decimal(&tmp1) <= tenth {
            use_power_series = true;
        } else {
            let float = f64_of(&z).ok_or(NumericError::InvalidOperation)?;
            tmp1 = decimal_from_f64(crate::gomath::log(float))
                .map(Decimal::into_inner)
                .ok_or(NumericError::InvalidOperation)?;
        }
    }

    if use_power_series {
        let tmp3 = add_at(
            &tmp1,
            &BigDecimal::from(2),
            precision,
            RoundingMode::HalfEven,
        )?;
        let ratio = quo_at(&tmp1, &tmp3, precision, RoundingMode::HalfEven)?;
        let doubled = add_at(&ratio, &ratio, precision, RoundingMode::HalfEven)?;
        let mut series = doubled.clone();
        let mut term = doubled;
        let epsilon = BigDecimal::new(
            BigInt::from(1),
            i64::try_from(precision).unwrap_or(i64::MAX),
        );
        let mut n = 1i64;
        loop {
            term = mul_at(&term, &ratio, precision, RoundingMode::HalfEven)?;
            term = mul_at(&term, &ratio, precision, RoundingMode::HalfEven)?;
            let divisor = BigDecimal::from(n.saturating_mul(2).saturating_add(1));
            let contribution = quo_at(&term, &divisor, precision, RoundingMode::HalfEven)?;
            series = add_at(&series, &contribution, precision, RoundingMode::HalfEven)?;
            if absolute_decimal(&contribution) <= epsilon {
                break;
            }
            n += 1;
        }
        tmp1 = series;
    } else {
        let loop_precision = final_precision.saturating_add(1);
        let mut previous: Option<BigDecimal> = None;
        for _ in 0..(10 + loop_precision) {
            let exp_value = apd_exp_at(&tmp1, precision, RoundingMode::HalfEven)?;
            let difference = add_at(
                &exp_value,
                &negate_raw(&z),
                precision,
                RoundingMode::HalfEven,
            )?;
            let numerator = add_at(&difference, &difference, precision, RoundingMode::HalfEven)?;
            let denominator = add_at(&exp_value, &z, precision, RoundingMode::HalfEven)?;
            let correction = quo_at(&numerator, &denominator, precision, RoundingMode::HalfEven)?;
            let next = add_at(
                &tmp1,
                &negate_raw(&correction),
                precision,
                RoundingMode::HalfEven,
            )?;
            if let Some(previous) = &previous {
                let delta = absolute_decimal(&add_at(
                    previous,
                    &negate_raw(&next),
                    precision,
                    RoundingMode::HalfEven,
                )?);
                if delta == 0 {
                    tmp1 = next;
                    break;
                }
                let exponent = -(i64::try_from(loop_precision).unwrap_or(i64::MAX))
                    + num_digits(&next)
                    + apd_exponent(&next);
                let epsilon = BigDecimal::new(BigInt::from(1), -exponent);
                if delta <= epsilon {
                    tmp1 = next;
                    break;
                }
            }
            previous = Some(next.clone());
            tmp1 = next;
        }
    }

    let result = add_at(&tmp1, &result_adjust, precision, RoundingMode::HalfEven)?;
    let rounded = round_with_mode(&result, final_precision, final_mode);
    check_exponent(&rounded)?;
    Ok(rounded)
}

/// Natural logarithm matching CUE's `math.Log` (34-digit, half-up final
/// rounding).
pub fn ln(value: &BigDecimal) -> NumericResult<BigDecimal> {
    apd_ln_at(value, CUE_PRECISION, RoundingMode::HalfUp)
}

/// `e^x` matching CUE's `math.Exp`.
pub fn exp(value: &BigDecimal) -> NumericResult<BigDecimal> {
    apd_exp_at(value, CUE_PRECISION, RoundingMode::HalfUp)
}

/// `x^y` matching `apd.Context.Pow` for CUE's context.
pub fn pow(base: &BigDecimal, exponent: &BigDecimal) -> NumericResult<BigDecimal> {
    if base == 0 && exponent == 0 {
        return Err(NumericError::InvalidOperation);
    }
    if exponent == 0 {
        return Ok(BigDecimal::from(1));
    }
    if base == 0 {
        if exponent > 0 {
            return Ok(BigDecimal::from(0));
        }
        return Err(NumericError::Overflow);
    }

    let integer = exponent.with_scale_round(0, RoundingMode::Down);
    let fraction = working_round(&(exponent - &integer), WORK_PRECISION);
    let negative_base = base < 0;
    if negative_base && fraction != 0 {
        return Err(NumericError::InvalidOperation);
    }

    let digits = u64::try_from(num_digits(base)).unwrap_or(u64::MAX);
    let mut precision = CUE_PRECISION.max(digits);
    precision = precision.saturating_add(10);

    let integer_coefficient: Option<BigInt> = integer.to_bigint();
    let mut integer_part = match integer_coefficient {
        Some(value) => integer_power_at(
            &absolute_decimal(base),
            &value,
            precision,
            RoundingMode::HalfUp,
        )?,
        None => BigDecimal::from(1),
    };

    if fraction != 0 {
        let logarithm = apd_ln_at(&absolute_decimal(base), precision, RoundingMode::HalfUp)?;
        let scaled = round_with_mode(&(logarithm * &fraction), precision, RoundingMode::HalfUp);
        let factor = apd_exp_at(&scaled, precision, RoundingMode::HalfUp)?;
        integer_part = mul_at(&integer_part, &factor, precision, RoundingMode::HalfUp)?;
    }

    // apd.Pow's final `round` rejects a raw exponent below MinExponent
    // before it drops digits, so `10^-99958` fails even though its adjusted
    // exponent is in range.  Quo deliberately does not use this check.
    if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&apd_exponent(&integer_part)) {
        return Err(NumericError::ExponentOutOfRange);
    }
    let mut result = round_with_mode(&integer_part, CUE_PRECISION, RoundingMode::HalfUp);
    check_exponent(&result)?;
    if negative_base
        && let Some(value) = exponent.to_bigint()
        && value.bit(0)
    {
        result = -result;
    }
    Ok(result)
}

fn integer_power_at(
    base: &BigDecimal,
    exponent: &BigInt,
    precision: u64,
    mode: RoundingMode,
) -> NumericResult<BigDecimal> {
    let negative = exponent.sign() == Sign::Minus;
    let mut power = absolute(exponent);
    let mut factor = base.clone();
    let mut result = BigDecimal::from(1);
    while power > BigInt::from(0) {
        if power.bit(0) {
            result = mul_at(&result, &factor, precision, mode)?;
        }
        power >>= 1;
        if power > BigInt::from(0) {
            factor = mul_at(&factor, &factor, precision, mode)?;
        }
    }
    if negative {
        result = divide_with_precision_mode(&BigDecimal::from(1), &result, precision, mode)?;
    }
    check_exponent(&result)?;
    Ok(result)
}

/// Cube root, ported from `apd.Context.Cbrt` (normalization by powers of 8,
/// a polynomial initial estimate, then Newton-Raphson).
pub fn cbrt(value: &BigDecimal) -> NumericResult<BigDecimal> {
    if value == 0 {
        return Ok(BigDecimal::from(0));
    }
    let negative = value < 0;
    let magnitude = absolute_decimal(value);
    let precision = CUE_PRECISION * 2 + 2;
    let mut z = magnitude.clone();
    let mut exponent8 = 0i64;
    let eighth = BigDecimal::new(BigInt::from(125), 3);
    let eight = BigDecimal::from(8);
    while z < eighth {
        exponent8 -= 1;
        z = mul_at(&z, &eight, precision, RoundingMode::HalfUp)?;
    }
    while z > 1 {
        exponent8 += 1;
        z = mul_at(&z, &eighth, precision, RoundingMode::HalfUp)?;
    }
    let z0 = z.clone();
    let c1: BigDecimal = "-0.46946116"
        .parse()
        .unwrap_or_else(|_| BigDecimal::from(0));
    let c2: BigDecimal = "1.072302".parse().unwrap_or_else(|_| BigDecimal::from(0));
    let c3: BigDecimal = "0.3812513".parse().unwrap_or_else(|_| BigDecimal::from(0));
    z = mul_at(&z, &c1, precision, RoundingMode::HalfUp)?;
    z = add_at(&z, &c2, precision, RoundingMode::HalfUp)?;
    z = mul_at(&z, &z0, precision, RoundingMode::HalfUp)?;
    z = add_at(&z, &c3, precision, RoundingMode::HalfUp)?;
    let half = BigDecimal::new(BigInt::from(5), 1);
    let two = BigDecimal::from(2);
    while exponent8 < 0 {
        z = mul_at(&z, &half, precision, RoundingMode::HalfUp)?;
        exponent8 += 1;
    }
    while exponent8 > 0 {
        z = mul_at(&z, &two, precision, RoundingMode::HalfUp)?;
        exponent8 -= 1;
    }

    let loop_precision = CUE_PRECISION + 1;
    let mut previous: Option<BigDecimal> = None;
    for _ in 0..(10 + loop_precision) {
        let z0 = z.clone();
        let squared = mul_at(&z, &z0, precision, RoundingMode::HalfUp)?;
        z = divide_with_precision_mode(&magnitude, &squared, precision, RoundingMode::HalfUp)?;
        z = add_at(&z, &z0, precision, RoundingMode::HalfUp)?;
        z = add_at(&z, &z0, precision, RoundingMode::HalfUp)?;
        z = divide_with_precision_mode(&z, &BigDecimal::from(3), precision, RoundingMode::HalfUp)?;
        if let Some(previous) = &previous {
            let delta = absolute_decimal(&add_at(
                previous,
                &negate_raw(&z),
                precision,
                RoundingMode::HalfUp,
            )?);
            if delta == 0 {
                break;
            }
            let exponent = -(i64::try_from(loop_precision).unwrap_or(i64::MAX))
                + num_digits(&z)
                + apd_exponent(&z);
            let epsilon = BigDecimal::new(BigInt::from(1), -exponent);
            if delta <= epsilon {
                break;
            }
        }
        previous = Some(z.clone());
    }

    let rounded = round_with_mode(&z, CUE_PRECISION, RoundingMode::HalfUp);
    check_exponent(&rounded)?;
    // apd checks exactness by computing d*d*d.  Mul rejects a raw exponent
    // sum below MinExponent even when the adjusted exponent is in range.
    if apd_exponent(&rounded).saturating_mul(3) < MIN_EXPONENT {
        return Err(NumericError::ExponentOutOfRange);
    }
    Ok(if negative { -rounded } else { rounded })
}

/// `log10` matching CUE's `math.Log10`: ln at 36 digits (half-even final)
/// multiplied by the 64-digit `1/ln(10)` constant at 34 digits half-even.
pub fn log10(value: &BigDecimal) -> NumericResult<BigDecimal> {
    let logarithm = apd_ln_at(value, CUE_PRECISION + 2, RoundingMode::HalfEven)?;
    let result = mul_at(
        &logarithm,
        &inv_ln10_const(),
        CUE_PRECISION,
        RoundingMode::HalfEven,
    )?;
    check_exponent(&result)?;
    Ok(result)
}

/// CUE's `math.Log2`: both logarithms are rounded to the 34-digit context
/// before the final reduced division.
pub fn log2(value: &BigDecimal) -> NumericResult<BigDecimal> {
    let numerator = ln(value)?;
    let denominator = ln(&BigDecimal::from(2))?;
    divide(&numerator, &denominator)
}

/// CUE's `math.Exp2`: `Pow(2, x)`.
pub fn exp2(value: &BigDecimal) -> NumericResult<BigDecimal> {
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
        let sum = add(&dec("12345678901234567890123456789012345"), &dec("1")).expect("add");
        assert_eq!(
            cue_decimal_string(&sum),
            "1.234567890123456789012345678901235E+34"
        );
        let product =
            mul(&dec("1234567890123456789012345678901234567890"), &dec("2")).expect("mul");
        assert_eq!(
            cue_decimal_string(&product),
            "2.469135780246913578024691357802469E+39"
        );
    }

    #[test]
    fn rounds_ties_away_from_zero() {
        assert_eq!(
            cue_decimal_string(
                &add(&dec("99999999999999999999999999999999985"), &dec("0")).expect("add")
            ),
            "9.999999999999999999999999999999999E+34"
        );
        assert_eq!(
            cue_decimal_string(
                &add(&dec("12345678901234567890123456789012345"), &dec("0")).expect("add")
            ),
            "1.234567890123456789012345678901235E+34"
        );
        assert_eq!(
            cue_decimal_string(&sub(
                &dec(
                    "-1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                ),
                &dec("1")
            ).expect("sub")),
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
        assert!(divide(&dec("1"), &dec("0")).is_err());
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
    fn matches_apd_log_family() {
        assert_eq!(
            cue_decimal_string(&ln(&dec("2e21")).expect("ln")),
            "49.04743413343490467379505266982983"
        );
        assert_eq!(
            cue_decimal_string(&log10(&dec("29e9")).expect("log10")),
            "10.46239799789895608733284676296926"
        );
        assert_eq!(
            cue_decimal_string(&log2(&dec("7e6")).expect("log2")),
            "22.73892349138177819466388589416818"
        );
    }

    #[test]
    fn enforces_apd_exponent_limits() {
        assert_eq!(
            add(&dec("1e100000"), &dec("1e-100000")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            divide(&dec("1e-100000"), &dec("3")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            mul(&dec("1e99999"), &dec("100")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(exp(&dec("23000")), Err(NumericError::Overflow));
        assert_eq!(exp(&dec("-23000")), Err(NumericError::Underflow));
        assert_eq!(
            pow(&dec("10"), &dec("100001")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            pow(&dec("10"), &dec("-99958")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            pow(&dec("10"), &dec("-99950.5")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            mul(&dec("1e-99999"), &dec("1e-2")),
            Err(NumericError::ExponentOutOfRange)
        );
        assert_eq!(
            cbrt(&dec("1e-100000")),
            Err(NumericError::ExponentOutOfRange)
        );
    }

    #[test]
    fn allows_apd_exponent_boundaries() {
        assert_eq!(
            cue_decimal_string(&add(&dec("1e100000"), &dec("0")).expect("add")),
            "1.000000000000000000000000000000000E+100000"
        );
        assert_eq!(
            cue_decimal_string(&mul(&dec("1e50000"), &dec("1e50000")).expect("mul")),
            "1E+100000"
        );
        assert_eq!(
            cue_decimal_string(&mul(&dec("1e99999"), &dec("10")).expect("mul")),
            "1.0E+100000"
        );
        assert_eq!(
            cue_decimal_string(&mul(&dec("1e-99999"), &dec("1e-1")).expect("mul")),
            "1E-100000"
        );
        assert_eq!(
            cue_decimal_string(&pow(&dec("10"), &dec("-99957")).expect("pow")),
            "1.000000000000000000000000000000000E-99957"
        );
    }

    #[test]
    fn converts_float64_like_apd() {
        assert_eq!(decimal_display(&decimal_from_f64(10.0).expect("f")), "1E+1");
        assert_eq!(
            decimal_display(&decimal_from_f64(1.4142135623730951).expect("f")),
            "1.4142135623730951"
        );
        assert_eq!(
            decimal_display(&decimal_from_f64(0.0000001).expect("f")),
            "1E-7"
        );
    }
}

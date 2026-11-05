#![allow(clippy::excessive_precision, clippy::approx_constant)]

//! Selected `math` functions ported from Go's standard library.
//!
//! CUE's `math` builtins that take or return `float64` call Go's `math`
//! package.  Platform libm implementations (which Rust's `f64` methods use)
//! differ from Go's Cephes-derived polynomial approximations in the last bit,
//! so matching the oracle requires the same algorithms.  The functions here
//! are direct translations of the Go 1.26 sources (BSD licensed), including
//! the Payne-Hanek argument reduction used by `Sin`/`Cos`/`Tan`.

const PI: f64 = 3.14159265358979323846264338327950288419716939937510582097494459;
const PI4A: f64 = 7.85398125648498535156e-1;
const PI4B: f64 = 3.77489470793079817668e-8;
const PI4C: f64 = 2.69515142907905952645e-15;

const SIN_COEFFICIENTS: [f64; 6] = [
    1.58962301576546568060e-10,
    -2.50507477628578072866e-8,
    2.75573136213857245213e-6,
    -1.98412698295895385996e-4,
    8.33333333332211858878e-3,
    -1.66666666666666307295e-1,
];

const COS_COEFFICIENTS: [f64; 6] = [
    -1.13585365213876817300e-11,
    2.08757008419747316778e-9,
    -2.75573141792967388112e-7,
    2.48015872888517045348e-5,
    -1.38888888888730564116e-3,
    4.16666666666665929218e-2,
];

const TAN_P: [f64; 3] = [
    -1.30936939181383777646e4,
    1.15351664838587416140e6,
    -1.79565251976484877988e7,
];

const TAN_Q: [f64; 5] = [
    1.00000000000000000000e0,
    1.36812963470692954678e4,
    -1.32089234440210967447e6,
    2.50083801823357915839e7,
    -5.38695755929454629881e7,
];

const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;

const MPI4: [u64; 20] = [
    0x0000_0000_0000_0001,
    0x45f3_06dc_9c88_2a53,
    0xf84e_afa3_ea69_bb81,
    0xb6c5_2b32_7887_2083,
    0xfca2_c757_bd77_8ac3,
    0x6e48_dc74_849b_a5c0,
    0x0c92_5dd4_13a3_2439,
    0xfc3b_d639_6253_4e7d,
    0xd104_6bea_5d76_8909,
    0xd338_e04d_68be_fc82,
    0x7323_ac73_06a6_73e9,
    0x3908_bf17_7bf2_5076,
    0x3ff1_2fff_bc0b_301f,
    0xde5e_2316_b414_da3e,
    0xda6c_fd9e_4f96_136e,
    0x9e8c_7ecd_3cbf_d45a,
    0xea4f_758f_d7cb_e2f6,
    0x7a0e_73ef_14a5_25d4,
    0xd7f6_bf62_3f1a_ba10,
    0xac06_608d_f8f6_d757,
];

fn mul64(a: u64, b: u64) -> (u64, u64) {
    // Mirrors `bits.Mul64`, which returns (hi, lo).
    let product = u128::from(a) * u128::from(b);
    ((product >> 64) as u64, product as u64)
}

fn add64(a: u64, b: u64, carry: u64) -> (u64, u64) {
    let sum = u128::from(a) + u128::from(b) + u128::from(carry);
    (sum as u64, (sum >> 64) as u64)
}

fn horner(coefficients: &[f64], z: f64) -> f64 {
    let mut value = coefficients[0];
    for coefficient in &coefficients[1..] {
        value = value.mul_add(z, *coefficient);
    }
    value
}

fn shl64(value: u64, shift: u32) -> u64 {
    if shift >= 64 { 0 } else { value << shift }
}

fn shr64(value: u64, shift: u32) -> u64 {
    if shift >= 64 { 0 } else { value >> shift }
}

/// Payne-Hanek range reduction by Pi/4 for `x > 0`.
fn trig_reduce(x: f64) -> (u64, f64) {
    const PI4: f64 = PI / 4.0;
    if x < PI4 {
        return (0, x);
    }
    let mut ix = x.to_bits();
    let exp = i64::from(((ix >> 52) & 0x7ff) as i32) - 1023 - 52;
    ix &= !(0x7ffu64 << 52);
    ix |= 1u64 << 52;

    let digit = usize::try_from((exp + 61) / 64).unwrap_or(0);
    let bitshift = u32::try_from((exp + 61) % 64).unwrap_or(0);
    let z0 = shl64(MPI4[digit], bitshift) | shr64(MPI4[digit + 1], 64 - bitshift);
    let z1 = shl64(MPI4[digit + 1], bitshift) | shr64(MPI4[digit + 2], 64 - bitshift);
    let z2 = shl64(MPI4[digit + 2], bitshift) | shr64(MPI4[digit + 3], 64 - bitshift);

    let (z2hi, _) = mul64(z2, ix);
    let (z1hi, z1lo) = mul64(z1, ix);
    let z0lo = z0.wrapping_mul(ix);
    let (lo, c) = add64(z1lo, z2hi, 0);
    let (mut hi, _) = add64(z0lo, z1hi, c);

    let mut j = hi >> 61;
    hi = (hi << 3) | (lo >> 61);
    let lz = hi.leading_zeros();
    let e = (1023u64).wrapping_sub(u64::from(lz) + 1);
    hi = shl64(hi, lz + 1) | shr64(lo, 64 - (lz + 1));
    hi >>= 64 - 52;
    hi |= e << 52;
    let mut z = f64::from_bits(hi);
    if j & 1 == 1 {
        j += 1;
        j &= 7;
        z -= 1.0;
    }
    (j, z * PI4)
}

/// Go's `math.Sin`.
#[must_use]
pub(crate) fn sin(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (mut j, z) = reduce(x);
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        let polynomial = horner(&COS_COEFFICIENTS, zz);
        (zz * zz).mul_add(polynomial, (-0.5f64).mul_add(zz, 1.0))
    } else {
        let polynomial = horner(&SIN_COEFFICIENTS, zz);
        (z * zz).mul_add(polynomial, z)
    };
    if sign { -y } else { y }
}

/// Go's `math.Cos`.
#[must_use]
pub(crate) fn cos(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let x = x.abs();
    let (mut j, z) = reduce(x);
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        let polynomial = horner(&SIN_COEFFICIENTS, zz);
        (z * zz).mul_add(polynomial, z)
    } else {
        let polynomial = horner(&COS_COEFFICIENTS, zz);
        (zz * zz).mul_add(polynomial, (-0.5f64).mul_add(zz, 1.0))
    };
    if sign { -y } else { y }
}

/// Go's `math.Tan`.
#[must_use]
pub(crate) fn tan(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (j, z) = reduce(x);
    let zz = z * z;
    let mut y = if zz > 1e-14 {
        let numerator = horner(&TAN_P, zz);
        let denominator = horner(&TAN_Q, zz);
        let inner = (zz * numerator) / denominator;
        z.mul_add(inner, z)
    } else {
        z
    };
    if j & 2 == 2 {
        y = -1.0 / y;
    }
    if sign { -y } else { y }
}

fn reduce(x: f64) -> (u64, f64) {
    if x >= REDUCE_THRESHOLD {
        return trig_reduce(x);
    }
    let mut j = (x * (4.0 / PI)) as u64;
    let mut y = j as f64;
    if j & 1 == 1 {
        j += 1;
        y += 1.0;
    }
    let z = ((x - y * PI4A) - y * PI4B) - y * PI4C;
    (j & 7, z)
}

fn xatan(x: f64) -> f64 {
    const P0: f64 = -8.750608600031904122785e-01;
    const P1: f64 = -1.615753718733365076637e+01;
    const P2: f64 = -7.500855792314704667340e+01;
    const P3: f64 = -1.228866684490136173410e+02;
    const P4: f64 = -6.485021904942025371773e+01;
    const Q0: f64 = 2.485846490142306297962e+01;
    const Q1: f64 = 1.650270098316988542046e+02;
    const Q2: f64 = 4.328810604912902668951e+02;
    const Q3: f64 = 4.853903996359136964868e+02;
    const Q4: f64 = 1.945506571482613964425e+02;
    let z = x * x;
    let numerator = horner(&[P0, P1, P2, P3, P4], z);
    let denominator = horner(&[1.0, Q0, Q1, Q2, Q3, Q4], z);
    let z = (z * numerator) / denominator;
    x.mul_add(z, x)
}

fn satan(x: f64) -> f64 {
    const MORE_BITS: f64 = 6.123233995736765886130e-17;
    const TAN3PIO8: f64 = 2.41421356237309504880;
    if x <= 0.66 {
        return xatan(x);
    }
    if x > TAN3PIO8 {
        return PI / 2.0 - xatan(1.0 / x) + MORE_BITS;
    }
    PI / 4.0 + xatan((x - 1.0) / (x + 1.0)) + 0.5 * MORE_BITS
}

/// Go's `math.Atan`.
#[must_use]
pub(crate) fn atan(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 { satan(x) } else { -satan(-x) }
}

/// Go's `math.Asin`.
#[must_use]
pub(crate) fn asin(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if x > 1.0 {
        return f64::NAN;
    }
    let temp = (-x).mul_add(x, 1.0).sqrt();
    let result = if x > 0.7 {
        PI / 2.0 - satan(temp / x)
    } else {
        satan(x / temp)
    };
    if sign { -result } else { result }
}

/// Go's `math.Acos`.
#[must_use]
pub(crate) fn acos(x: f64) -> f64 {
    PI / 2.0 - asin(x)
}

/// Go's `math.Atan2`.
#[must_use]
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return 0.0f64.copysign(y);
        }
        return PI.copysign(y);
    }
    if x == 0.0 {
        return (PI / 2.0).copysign(y);
    }
    if x.is_infinite() {
        if x > 0.0 {
            if y.is_infinite() {
                return (PI / 4.0).copysign(y);
            }
            return 0.0f64.copysign(y);
        }
        if y.is_infinite() {
            return (3.0 * PI / 4.0).copysign(y);
        }
        return PI.copysign(y);
    }
    if y.is_infinite() {
        return (PI / 2.0).copysign(y);
    }
    let q = atan(y / x);
    if x < 0.0 {
        if q <= 0.0 {
            return q + PI;
        }
        return q - PI;
    }
    q
}

fn frexp(f: f64) -> (f64, i32) {
    if f == 0.0 || f.is_nan() || f.is_infinite() {
        return (f, 0);
    }
    let bits = f.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    if exponent == 0 {
        // Subnormal: scale into the normal range first.
        let (mantissa, extra) = frexp(f * 2f64.powi(64));
        return (mantissa, extra - 64);
    }
    let mantissa = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (mantissa, exponent - 1022)
}

fn ldexp(f: f64, mut exp: i32) -> f64 {
    if f == 0.0 || f.is_nan() || f.is_infinite() {
        return f;
    }
    let mut frac = f;
    if frac.abs() < f64::MIN_POSITIVE {
        frac *= (1u64 << 52) as f64;
        exp -= 52;
    }
    let bits = frac.to_bits();
    exp += ((bits >> 52) & 0x7ff) as i32 - 1023;
    if exp < -1075 {
        return 0.0f64.copysign(frac);
    }
    if exp > 1023 {
        return f64::INFINITY.copysign(frac);
    }
    let mut m = 1.0;
    if exp < -1022 {
        exp += 53;
        m = 1.0 / (1u64 << 53) as f64;
    }
    let x = (bits & !(0x7ffu64 << 52)) | (((exp + 1023) as u64) << 52);
    m * f64::from_bits(x)
}

/// Go's `math.Mod`.
#[must_use]
pub(crate) fn modulo(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() || x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let y = y.abs();
    let (yfr, yexp) = frexp(y);
    let mut r = x.abs();
    while r >= y {
        let (rfr, rexp) = frexp(r);
        let rexp = if rfr < yfr { rexp - 1 } else { rexp };
        r -= ldexp(y, rexp - yexp);
    }
    if x < 0.0 { -r } else { r }
}

/// Go's `math.Remainder`.
#[must_use]
pub(crate) fn remainder(x: f64, y: f64) -> f64 {
    const TINY: f64 = 4.45014771701440276618e-308;
    const HALF_MAX: f64 = f64::MAX / 2.0;
    if x.is_nan() || y.is_nan() || x.is_infinite() || y == 0.0 {
        return f64::NAN;
    }
    if y.is_infinite() {
        return x;
    }
    let sign = x < 0.0;
    let mut x = x.abs();
    let mut y = y.abs();
    if x == y {
        return if sign { -0.0 } else { 0.0 };
    }
    if y <= HALF_MAX {
        x = modulo(x, y + y);
    }
    if y < TINY {
        if x + x > y {
            x -= y;
            if x + x >= y {
                x -= y;
            }
        }
    } else {
        let y_half = 0.5 * y;
        if x > y_half {
            x -= y;
            if x >= y_half {
                x -= y;
            }
        }
    }
    let _ = &mut y;
    if sign { -x } else { x }
}

const SQRT2: f64 = 1.4142135623730951;
const LN2: f64 = 0.693147180559945309417232121458176568;

/// Go's `math.Log`.
#[must_use]
pub(crate) fn log(x: f64) -> f64 {
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const L1: f64 = 6.666666666666735130e-01;
    const L2: f64 = 3.999999999940941908e-01;
    const L3: f64 = 2.857142874366239149e-01;
    const L4: f64 = 2.222219843214978396e-01;
    const L5: f64 = 1.818357216161805012e-01;
    const L6: f64 = 1.531383769920937332e-01;
    const L7: f64 = 1.479819860511658591e-01;

    if x.is_nan() || (x.is_infinite() && x > 0.0) {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let (mut f1, mut ki) = frexp(x);
    if f1 < SQRT2 / 2.0 {
        f1 *= 2.0;
        ki -= 1;
    }
    let f = f1 - 1.0;
    let k = f64::from(ki);
    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    let t1 = s2 * (L1 + s4 * (L3 + s4 * (L5 + s4 * L7)));
    let t2 = s4 * (L2 + s4 * (L4 + s4 * L6));
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;
    k * LN2_HI - ((hfsq - (s * (hfsq + r) + k * LN2_LO)) - f)
}

/// Go's `math.Log1p`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub(crate) fn log1p(x: f64) -> f64 {
    const SQRT2M1: f64 = 4.142135623730950488017e-01;
    const SQRT2HALFM1: f64 = -2.928932188134524755992e-01;
    const SMALL: f64 = 1.0 / (1u64 << 29) as f64;
    const TINY: f64 = 1.0 / (1u64 << 54) as f64;
    const TWO53: f64 = (1u64 << 53) as f64;
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const LP1: f64 = 6.666666666666735130e-01;
    const LP2: f64 = 3.999999999940941908e-01;
    const LP3: f64 = 2.857142874366239149e-01;
    const LP4: f64 = 2.222219843214978396e-01;
    const LP5: f64 = 1.818357216161805012e-01;
    const LP6: f64 = 1.531383769920937332e-01;
    const LP7: f64 = 1.479819860511658591e-01;

    if x < -1.0 || x.is_nan() {
        return f64::NAN;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() && x > 0.0 {
        return x;
    }
    let absx = x.abs();
    let mut f = 0.0;
    let mut iu = 0u64;
    let mut k = 1i32;
    if absx < SQRT2M1 {
        if absx < SMALL {
            if absx < TINY {
                return x;
            }
            return x - x * x * 0.5;
        }
        if x > SQRT2HALFM1 {
            k = 0;
            f = x;
            iu = 1;
        }
    }
    let mut c = 0.0;
    if k != 0 {
        let mut u;
        if absx < TWO53 {
            u = 1.0 + x;
            iu = u.to_bits();
            k = i32::try_from(iu >> 52).unwrap_or(i32::MAX) - 1023;
            c = if k > 0 { 1.0 - (u - x) } else { x - (u - 1.0) };
            c /= u;
        } else {
            u = x;
            iu = u.to_bits();
            k = i32::try_from((iu >> 52) & 0x7ff).unwrap_or(0) - 1023;
            c = 0.0;
        }
        iu &= 0x000f_ffff_ffff_ffff;
        if iu < 0x0006_a09e_667f_3bcd {
            u = f64::from_bits(iu | 0x3ff0_0000_0000_0000);
        } else {
            k += 1;
            u = f64::from_bits(iu | 0x3fe0_0000_0000_0000);
            iu = (0x0010_0000_0000_0000 - iu) >> 2;
        }
        f = u - 1.0;
    }
    let hfsq = 0.5 * f * f;
    if iu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c += f64::from(k) * LN2_LO;
            return f64::from(k) * LN2_HI + c;
        }
        let r = hfsq * (1.0 - 0.66666666666666666 * f);
        if k == 0 {
            return f - r;
        }
        return f64::from(k) * LN2_HI - ((r - (f64::from(k) * LN2_LO + c)) - f);
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let r = z * (LP1 + z * (LP2 + z * (LP3 + z * (LP4 + z * (LP5 + z * (LP6 + z * LP7))))));
    if k == 0 {
        return f - (hfsq - s * (hfsq + r));
    }
    (f64::from(k) * LN2_HI - (hfsq - (s * (hfsq + r) + (f64::from(k) * LN2_LO + c)))) + f
}

/// Go's `math.Exp`.
#[must_use]
pub(crate) fn exp(x: f64) -> f64 {
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const LOG2E: f64 = 1.44269504088896338700e+00;
    const OVERFLOW: f64 = 7.09782712893383973096e+02;
    const UNDERFLOW: f64 = -7.45133219101941108420e+02;
    const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;

    if x.is_nan() {
        return x;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    if x < UNDERFLOW {
        return 0.0;
    }
    if -NEAR_ZERO < x && x < NEAR_ZERO {
        return 1.0 + x;
    }
    let k = if x < 0.0 {
        (LOG2E * x - 0.5) as i32
    } else if x > 0.0 {
        (LOG2E * x + 0.5) as i32
    } else {
        0
    };
    let hi = x - f64::from(k) * LN2_HI;
    let lo = f64::from(k) * LN2_LO;
    expmulti(hi, lo, k)
}

fn expmulti(hi: f64, lo: f64, k: i32) -> f64 {
    const P1: f64 = 1.66666666666666657415e-01;
    const P2: f64 = -2.77777777770155933842e-03;
    const P3: f64 = 6.61375632143793436117e-05;
    const P4: f64 = -1.65339022054652515390e-06;
    const P5: f64 = 4.13813679705723846039e-08;
    let r = hi - lo;
    let t = r * r;
    let inner = horner(&[P5, P4, P3, P2, P1], t);
    let c = (-t).mul_add(inner, r);
    let y = 1.0 - ((lo - (r * c) / (2.0 - c)) - hi);
    ldexp(y, k)
}

/// Go's `math.Sinh`.
#[must_use]
pub(crate) fn sinh(x: f64) -> f64 {
    const P0: f64 = -0.6307673640497716991184787251e+6;
    const P1: f64 = -0.8991272022039509355398013511e+5;
    const P2: f64 = -0.2894211355989563807284660366e+4;
    const P3: f64 = -0.2630563213397497062819489e+2;
    const Q0: f64 = -0.6307673640497716991212077277e+6;
    const Q1: f64 = 0.1521517378790019070696485176e+5;
    const Q2: f64 = -0.173678953558233699533450911e+3;
    let sign = x < 0.0;
    let x = x.abs();
    let temp = if x > 21.0 {
        exp(x) * 0.5
    } else if x > 0.5 {
        let ex = exp(x);
        (ex - 1.0 / ex) * 0.5
    } else {
        let sq = x * x;
        let numerator = horner(&[P3, P2, P1, P0], sq);
        let denominator = horner(&[1.0, Q2, Q1, Q0], sq);
        (numerator * x) / denominator
    };
    if sign { -temp } else { temp }
}

/// Go's `math.Cosh`.
#[must_use]
pub(crate) fn cosh(x: f64) -> f64 {
    let x = x.abs();
    if x > 21.0 {
        return exp(x) * 0.5;
    }
    let ex = exp(x);
    (ex + 1.0 / ex) * 0.5
}

/// Go's `math.Tanh`.
#[must_use]
pub(crate) fn tanh(x: f64) -> f64 {
    const MAXLOG: f64 = 8.8029691931113054295988e+01;
    const P: [f64; 3] = [
        -9.64399179425052238628e-1,
        -9.92877231001918586564e1,
        -1.61468768441708447952e3,
    ];
    const Q: [f64; 3] = [
        1.12811678491632931402e2,
        2.23548839060100448583e3,
        4.84406305325125486048e3,
    ];
    let z = x.abs();
    if z > 0.5 * MAXLOG {
        return if x < 0.0 { -1.0 } else { 1.0 };
    }
    if z >= 0.625 {
        let s = exp(2.0 * z);
        let mut result = 1.0 - 2.0 / (s + 1.0);
        if x < 0.0 {
            result = -result;
        }
        return result;
    }
    if x == 0.0 {
        return x;
    }
    let s = x * x;
    let numerator = horner(&P, s);
    let denominator = horner(&[1.0, Q[0], Q[1], Q[2]], s);
    x + ((x * s) * numerator) / denominator
}

/// Go's `math.Asinh`.
#[must_use]
pub(crate) fn asinh(x: f64) -> f64 {
    const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;
    const LARGE: f64 = (1u64 << 28) as f64;
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let sign = x < 0.0;
    let x = x.abs();
    let temp = if x > LARGE {
        log(x) + LN2
    } else if x > 2.0 {
        log(2.0 * x + 1.0 / ((x * x + 1.0).sqrt() + x))
    } else if x < NEAR_ZERO {
        x
    } else {
        log1p(x + x * x / (1.0 + (1.0 + x * x).sqrt()))
    };
    if sign { -temp } else { temp }
}

/// Go's `math.Acosh`.
#[must_use]
pub(crate) fn acosh(x: f64) -> f64 {
    const LARGE: f64 = (1u64 << 28) as f64;
    if x < 1.0 || x.is_nan() {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x >= LARGE {
        return log(x) + LN2;
    }
    if x > 2.0 {
        let root = x.mul_add(x, -1.0).sqrt();
        return log(2.0f64.mul_add(x, -(1.0 / (x + root))));
    }
    let t = x - 1.0;
    log1p(t + (2.0 * t + t * t).sqrt())
}

/// Go's `math.Atanh`.
#[must_use]
pub(crate) fn atanh(x: f64) -> f64 {
    const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;
    if !(-1.0..=1.0).contains(&x) || x.is_nan() {
        return f64::NAN;
    }
    if x == 1.0 {
        return f64::INFINITY;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    let sign = x < 0.0;
    let x = x.abs();
    let temp = if x < NEAR_ZERO {
        x
    } else if x < 0.5 {
        let temp = x + x;
        0.5 * log1p(temp + temp * x / (1.0 - x))
    } else {
        0.5 * log1p((x + x) / (1.0 - x))
    };
    if sign { -temp } else { temp }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_go_trigonometry() {
        assert_eq!(sin(1.0), 0.841_470_984_807_896_5);
        assert_eq!(cos(1.0), 0.540_302_305_868_139_8);
        assert_eq!(tan(1.0), 1.557_407_724_654_902);
        assert_eq!(asin(0.5), 0.523_598_775_598_298_9);
        assert_eq!(acos(0.5), 1.047_197_551_196_597_6);
        assert_eq!(atan(1.0), 0.785_398_163_397_448_3);
        assert_eq!(atan2(1.0, -2.0), 2.677_945_044_588_987);
    }

    #[test]
    fn matches_go_hyperbolics() {
        assert_eq!(exp(40.0), 2.3538526683701997e17);
        assert_eq!(sinh(40.0), 1.1769263341850998e17);
        assert_eq!(cosh(40.0), 1.1769263341850998e17);
        assert_eq!(tanh(40.0), 1.0);
        assert_eq!(sinh(709.7), 8.274920138401322e307);
        assert_eq!(sinh(-709.7), -8.274920138401322e307);
        assert_eq!(cosh(709.437), 6.361277807292747e307);
        assert_eq!(cosh(709.78), 8.964113971972578e307);
    }

    #[test]
    fn matches_go_modulo_and_remainder() {
        let value = 1.234_567_890_123_456_8e39;
        assert_eq!(modulo(value, 7.0), 4.0);
        assert_eq!(remainder(value, 7.0), -3.0);
    }
}

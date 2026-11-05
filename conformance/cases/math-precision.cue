package mathprecision

import "math"

params: {
	// Decimal-context functions (34 significant digits).
	exp1:    math.Exp(1)
	exp2:    math.Exp(2)
	expneg:  math.Exp(-1)
	log2:    math.Log(2)
	log10:   math.Log(10)
	log10b:  math.Log10(2)
	log2b:   math.Log2(3)
	log_large:  math.Log(2e21)
	log10_large: math.Log10(29e9)
	log2_large:  math.Log2(7e6)
	pow_frac: math.Pow(2, 0.5)
	pow_11:   math.Pow(1.1, 100)
	pow_neg:  math.Pow(3, -2.5)
	pow_big:  math.Pow(10, 400)
	cbrt2:    math.Cbrt(2)
	cbrt10:   math.Cbrt(10)
	cbrtneg:  math.Cbrt(-8)
	exp2b:    math.Exp2(10)
	exp2_low:   math.Exp2(-332049)
	pow_low:    math.Pow(10, -99957)
	pow_frac_low: math.Pow(10, -99913.5)

	// Sqrt is a correctly-rounded hardware operation and is architecture-stable.
	// Float64 transcendentals (Sin/Cos/Tan/Asin/Acos/Atan/Atan2/Sinh/Cosh/Tanh/
	// Asinh/Acosh/Atanh/Mod/Remainder) are Rust-only tests because Go's f64 math
	// is architecture-dependent (arm64 fuses multiply-adds).
	sqrt2:   math.Sqrt(2)
	sqrt4:   math.Sqrt(4)

	// String interpolation uses the same decimal representation as export.
	exp_interp: "\(math.Exp(1))"
	pow_interp: "\(math.Pow(2, 0.5))"
}

workflow: {
	name: "math-precision"
	steps: {s: {kind: "noop"}}
}

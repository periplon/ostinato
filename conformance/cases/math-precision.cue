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
	pow_frac: math.Pow(2, 0.5)
	pow_11:   math.Pow(1.1, 100)
	pow_neg:  math.Pow(3, -2.5)
	pow_big:  math.Pow(10, 400)
	cbrt2:    math.Cbrt(2)
	cbrt10:   math.Cbrt(10)
	cbrtneg:  math.Cbrt(-8)
	exp2b:    math.Exp2(10)

	// float64-backed functions follow Go's math package exactly.
	sqrt2:   math.Sqrt(2)
	sqrt4:   math.Sqrt(4)
	sin1:    math.Sin(1)
	cos1:    math.Cos(1)
	tan1:    math.Tan(1)
	sin075:  math.Sin(0.75)
	sinbig:  math.Sin(1e9)
	cosbig:  math.Cos(1e15)
	tanbig:  math.Tan(1e15)
	asin:    math.Asin(0.5)
	acos:    math.Acos(0.5)
	atan:    math.Atan(1)
	atan2:   math.Atan2(1, -2)
	sinh2:   math.Sinh(2)
	cosh2:   math.Cosh(2)
	tanh2:   math.Tanh(2)
	asinh:   math.Asinh(2)
	acosh:   math.Acosh(2)
	atanh:   math.Atanh(0.5)
	mod:     math.Mod(1234567890123456789012345678901234567890, 7)
	rem:     math.Remainder(1234567890123456789012345678901234567890, 7)

	// String interpolation uses the same decimal representation as export.
	exp_interp: "\(math.Exp(1))"
	pow_interp: "\(math.Pow(2, 0.5))"
	sin_interp: "\(math.Sin(1))"
}

workflow: {
	name: "math-precision"
	steps: {s: {kind: "noop"}}
}

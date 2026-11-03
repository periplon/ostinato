package mathintegral

import "math"

params: {
	sqrt:       math.Sqrt(4)
	sqrt_float: math.Sqrt(4) & float
	sqrt_two:   math.Sqrt(2)
	sqrt_zero:  math.Sqrt(0)
	pow:        math.Pow(2, 3)
	exp:        math.Exp(0)
	log:        math.Log(1)
	floor:      math.Floor(2.7)
	ceil:       math.Ceil(2.1)
	round:      math.Round(2.5)
	trunc:      math.Trunc(2.9)
	abs:        math.Abs(-2)
	abs_float:     math.Abs(-2.0)
	abs_float_int: math.Abs(-2.0) & int
	pow53:         math.Pow(2, 53) & int
	pow60:         math.Pow(2, 60)
	sqrt_mul:      math.Sqrt(4) * 2
	sqrt_sub:      math.Sqrt(4) - 1
	sqrt_div:      math.Sqrt(4) / 2
	sqrt_add:      math.Sqrt(4) + 1
	zero_add:      1.0 + (-1.0)
	zero_mul:      1.0 * 0.0
}

workflow: {
	name: "math-integral"
	steps: {s: {kind: "noop"}}
}

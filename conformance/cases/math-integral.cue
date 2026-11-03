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
}

workflow: {
	name: "math-integral"
	steps: {s: {kind: "noop"}}
}

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
	zero_mul_s:    "\(1.0 * 0.0)"
	one_mul_s:     "\(1.00 * 1.00)"
	abs_scale_s:   "\(math.Abs(-2.50))"
	two_mul_s:     "\(1.0 * 2.00)"
	exp_lit_s:     "\(1e2)"
	exp_add_s:     "\(1e2 + 2)"
	exp_mul_s:     "\(1e2 * 1.0)"
	exp_sub_s:     "\(1e2 - 1e2)"
	exp_frac_s:    "\(1e-2)"
	exp_15_s:      "\(1.5e2)"
	exp_div_one_s:  "\(1e2 / 1)"
	exp_div_two_s:  "\(1e2 / 2)"
	exp_div_half_s: "\(1e2 / 0.5)"
	exp_div_neg_s:  "\(1e2 / 1e-2)"
	exp_div_zero_s: "\(0e2 / 2)"
	dec_div_scale_s: "\(1.00 / 2)"
	sqrt_exp_s:     "\(math.Sqrt(1e2))"
	sqrt_exp4_s:    "\(math.Sqrt(4e2))"
	sqrt_exp8_s:    "\(math.Sqrt(1e4))"
	sqrt_exp100_s:  "\(math.Sqrt(1e100))"
}

workflow: {
	name: "math-integral"
	steps: {s: {kind: "noop"}}
}

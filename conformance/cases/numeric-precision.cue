package numericprecision

// CUE evaluates arithmetic in a 34-significant-digit decimal context.  The
// literals below stay exact; only arithmetic results are rounded.
d30:  123456789012345678901234567890
d34:  1234567890123456789012345678901234
d35:  12345678901234567890123456789012345
d100: 1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000

tie_down_value: 99999999999999999999999999999999985 + 0
tie_up_value:   99999999999999999999999999999999995 + 0
tie_mul_value: 12345678901234567890123456789012355 * 1

params: {
	add30:      d30 + 1
	sub30:      d30 - 1
	mul30:      d30 * 2
	div30:      d30 / 7
	add34:      d34 + 1
	add35:      d35 + 1
	sub35:      d35 - 1
	mul35:      d35 * 2
	add100:     d100 + 1
	sub100:     d100 - 1
	mul100:     d100 * 7
	div100:     d100 / 3
	neg100:     -d100 + 1
	tie_even_down: tie_down_value
	tie_even_up:   tie_up_value
	tie_mul:       tie_mul_value
	round_eq_true:  (12345678901234567890123456789012345 * 1) == 12345678901234567890123456789012350
	round_eq_false: (12345678901234567890123456789012345 * 1) == 12345678901234567890123456789012340
	// Division uses CUE's 34-digit expansion and keeps a fractional zero.
	third:       1 / 3
	two_thirds:  2 / 3
	seventh:     1 / 7
	hundred_div: 100 / 3
	half:        1 / 2
	four_div:    4 / 2
	zero:        0 / 3
	zero_scale:  0.00 / 2
	zero_exp:    0e2 / 2
	neg_third:   -1 / 3
	big_div:     1234567890123456789012345678901234567890 / 7
	// Dependent fields see the rounded value.
	chain: {
		base: d35
		next: base + 1
		again: next + 1
	}
	// JSON number formatting keeps the decimal exponent.
	lit_exp:   1E+400
	lit_frac:  1.2300
	lit_zero:  0.000
	lit_small: 1e-38
	lit_over_exp:  1.2300e100001
	lit_under_exp: 1e-100001
	mul_scale: 1.0 * 2.00
	// apd exponent boundary values that remain in range.
	exp_boundary_add:  1e100000 + 0
	exp_boundary_mul:  1e99999 * 10
	exp_boundary_pair: 1e50000 * 1e50000
	// Interpolation and type checks see the same kind and digits.
	interp_big:  "\(d35 + 1)"
	interp_div:  "\(1 / 3)"
	interp_math: "\(1.2300 * 1)"
	int_kind:    (d35 + 1) & int
	float_kind:  (10 / 2) & float
}

workflow: {
	name: "numeric-precision"
	steps: {s: {kind: "noop"}}
}

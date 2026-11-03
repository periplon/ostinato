package bigint

import "math"

big: 123456789012345678901234567890
neg: -123456789012345678901234567890

params: {
	literal:     big
	negative:    neg
	add:         big + 1
	sub:         big - 1
	mul:         big * 2
	div:         big / 2
	small_add:   1 + big
	greater:     big > 1
	less:        1 < big
	equal:       big == big
	not_equal:   big == 1
	unified_int: int & big
	unified_bnd: >=1 & big
	abs:         math.Abs(neg)
	interp:      "\(big)"
	list:        [big, neg]
	small_eq:    9223372036854775807 == 9223372036854775807
	over_i64:    9223372036854775808
}

workflow: {
	name: "big-int"
	steps: {s: {kind: "noop"}}
}

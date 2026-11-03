package bigint

import "math"
import "regexp"
import "time"
import "list"

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
	big_list:    [big, neg]
	small_eq:    9223372036854775807 == 9223372036854775807
	over_i64:    9223372036854775808
	bounded_big: (>=9223372036854775808 & <=9223372036854775810) & 9223372036854775809
	not_equal_big: (int & !=123456789012345678901234567890) & 5
	sqrt_big:    math.Sqrt(9223372036854775808)
	pow_big:     math.Pow(9223372036854775808, 1)
	find_all_big: regexp.FindAll("a", "aaa", 9223372036854775808)
	duration_big: time.FormatDuration(9223372036854775808)
	range_big:   (list.Range(9223372036854775808, 9223372036854775811, 1)[0]) & int
}

workflow: {
	name: "big-int"
	steps: {s: {kind: "noop"}}
}

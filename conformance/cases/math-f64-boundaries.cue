package mathf64boundaries

// Go's float64 transcendental math is architecture-dependent (arm64 uses
// fused multiply-add while amd64 generally does not). The boundary and
// Mod/Remainder cases therefore live in Rust-only tests
// (`crates/ostinato-lang/tests/math_f64_boundaries.rs`) instead of this
// oracle-compared corpus. Only architecture-stable exact values remain here.

params: {
	exact_int: 42
	exact_big: 1234567890123456789012345678901234567890 + 1
	exact_mul: 12345678901234567890123456789012345 * 2
	exact_div: 1 / 3
	exact_tie: 99999999999999999999999999999999985 + 0
}

workflow: {
	name: "math-f64-boundaries"
	steps: {s: {kind: "noop"}}
}

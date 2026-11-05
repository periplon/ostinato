#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

const SOURCE: &str = include_str!("fixtures/math-f64-boundaries.cue");
const EXPECTED: &str = include_str!("fixtures/math-f64-boundaries.json");

/// Go's `math` float64 results are architecture-dependent (arm64 fuses
/// multiply-adds). The Rust evaluator intentionally matches arm64 Go, so these
/// boundary and Mod/Remainder cases live here rather than in the differential
/// CUE conformance corpus.
#[test]
fn matches_arm64_go_f64_expectations() {
    let file = ostinato_lang::parse(SOURCE).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "math".to_owned(),
        ostinato_lang::builtin_module("math").expect("math builtin"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let actual = value.to_json().expect("concrete");
    let expected: serde_json::Value = serde_json::from_str(EXPECTED).expect("expected json");
    assert_eq!(actual, expected);

    assert_eq!(actual["sin075"].as_f64(), Some(0.6816387600233341));
    assert_eq!(actual["tanbig"].as_f64(), Some(-1.6724147821275834));
    assert_eq!(actual["sinh40"].as_f64(), Some(1.1769263341850998e17));
    assert_eq!(actual["cosh40"].as_f64(), Some(1.1769263341850998e17));
    assert_eq!(
        actual["sinh_internal"].as_f64(),
        Some(1.5229731293746392e96)
    );
    assert_eq!(
        actual["sinh_negative_internal"].as_f64(),
        Some(-938626257138334.2)
    );
    assert_eq!(actual["asinh_repro"].as_f64(), Some(-0.41180976518071905));
    assert_eq!(
        actual["acosh_repro_2ulp_a"].as_f64(),
        Some(0.775497469189701)
    );
    assert_eq!(actual["atanh_repro"].as_f64(), Some(-0.9938664724566799));
    for field in [
        "sinh_700",
        "cosh_700",
        "sinh_neg700",
        "cosh_neg700",
        "sinh_709",
        "cosh_709",
        "sinh_neg709",
        "cosh_neg709",
    ] {
        assert!(
            actual[field].as_f64().is_some_and(f64::is_finite),
            "{field} must stay finite: {}",
            actual[field]
        );
    }
}

#[test]
fn f64_model_is_pinned_and_fused_across_platforms() {
    assert_eq!(ostinato_lang::GO_F64_MODEL, "arm64-go-fma");
    // Distinguishes a fused multiply-add from separate multiply+add. `mul_add`
    // is fused on every Rust target (software `fma` where the CPU lacks FMA),
    // which is what pins the ported Go/arm64 arithmetic.
    let left = 1.0 + f64::EPSILON;
    let right = 1.0 - f64::EPSILON;
    let fused = left.mul_add(right, -1.0);
    let separate = left * right - 1.0;
    assert_ne!(fused, separate, "fused={fused:?} separate={separate:?}");
}

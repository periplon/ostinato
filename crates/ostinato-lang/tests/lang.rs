#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::{JoinMode, StepKind};
use ostinato_lang::{compile, eval_condition, eval_file, parse, render};
use serde_json::json;

const SOURCE: &str = r#"
package demo

params: {
    goal: string | *"ship it"
    retries: int | *2
}

#Retry: {
    max: int | *2
    baseMs: int | *5
}

workflow: {
    name: "demo"
    maxParallel: 2
    steps: {
        a: {
            kind: "noop"
            retry: #Retry
        }
        b: {
            kind:    "exec"
            needs:   ["a"]
            exec: {
                cmd: ["echo", "goal=\(params.goal)", "retries=\(params.retries)"]
            }
        }
    }
}
"#;

#[test]
fn evaluates_structs_defaults_unification_and_interpolation() {
    let file = parse(SOURCE).expect("parse");
    let value = eval_file(&file).expect("eval");
    let json = value.to_json().expect("concrete");
    assert_eq!(json["params"]["goal"], json!("ship it"));
    assert_eq!(json["params"]["retries"], json!(2));
    assert_eq!(json["workflow"]["steps"]["a"]["retry"]["max"], json!(2));
}

#[test]
fn compiles_workflow_and_resolves_parameters() {
    let compiled = compile(SOURCE).expect("compile");
    assert_eq!(compiled.spec.name, "demo");
    assert_eq!(compiled.spec.max_parallel, 2);
    let b = compiled.spec.step("b").expect("b");
    assert_eq!(b.kind, StepKind::Exec);
    assert_eq!(b.needs, vec!["a".to_owned()]);
    let a = compiled.spec.step("a").expect("a");
    assert_eq!(a.kind, StepKind::Noop);
    assert_eq!(a.retry.max_retries, 2);
    assert_eq!(a.retry.base_delay_ms, 5);

    let resolved = compiled
        .resolve_params(&BTreeMap::from([("goal".to_owned(), "prod".to_owned())]))
        .expect("params");
    assert_eq!(resolved["goal"], json!("prod"));
    assert_eq!(resolved["retries"], json!(2));
}

#[test]
fn rejects_missing_required_parameter() {
    let source = r#"
package demo
params: { goal: string }
workflow: { steps: { a: { kind: "noop" } } }
"#;
    let compiled = compile(source).expect("compile");
    let error = compiled
        .resolve_params(&BTreeMap::new())
        .expect_err("must fail");
    assert!(error.to_string().contains("missing required parameter"));
}

#[test]
fn evaluates_conditions() {
    let context = json!({
        "steps": { "review": { "score": 0.93, "status": "ok" } },
        "round": 2
    });
    assert!(eval_condition("steps.review.score >= 0.9", &context).expect("cond"));
    assert!(eval_condition("steps.review.status == \"ok\" && round == 2", &context).expect("cond"));
    assert!(!eval_condition("round < 1 || steps.review.score > 0.99", &context).expect("cond"));
}

#[test]
fn renders_templates() {
    let context = json!({
        "params": { "goal": "ship" },
        "steps": { "plan": { "stdout": "do it", "json": { "n": 3 } } }
    });
    assert_eq!(
        render(
            "goal={{ params.goal }} plan={{ steps.plan.stdout }} n={{ steps.plan.json.n }}",
            &context
        )
        .expect("render"),
        "goal=ship plan=do it n=3"
    );
}

fn template_context() -> serde_json::Value {
    json!({
        "run_id": "run-1",
        "workflow": "demo",
        "round": 2,
        "params": {
            "repo": "acme/app",
            "empty": "",
            "flag": false
        },
        "steps": {
            "build": {
                "stdout": "built ok",
                "stderr": "",
                "exitCode": 0,
                "score": 0.75,
                "json": {
                    "n": 3,
                    "ok": true,
                    "items": ["a", "b", "c"]
                }
            }
        }
    })
}

fn template_error(template: &str) -> String {
    render(template, &template_context())
        .expect_err("template must fail")
        .to_string()
}

#[test]
fn renders_dot_root_and_legacy_template_paths() {
    let context = template_context();
    assert_eq!(
        render(
            "{{ run_id }}|{{ .steps.build.stdout }}|{{ $.params.repo }}|{{ steps.build.stdout }}",
            &context
        )
        .expect("render"),
        "run-1|built ok|acme/app|built ok"
    );
    assert_eq!(
        render("{{ $ }}", &json!({"a": 1})).expect("render"),
        r#"{"a":1}"#
    );
}

#[test]
fn renders_the_current_dot_for_any_value() {
    assert_eq!(render("{{ . }}", &json!("text")).expect("render"), "text");
    assert_eq!(render("{{ . }}", &json!(3)).expect("render"), "3");
    assert_eq!(render("{{ . }}", &json!(true)).expect("render"), "true");
    assert_eq!(render("{{ . }}", &json!(null)).expect("render"), "");
    assert_eq!(render("{{ . }}", &json!([1, 2])).expect("render"), "[1,2]");
}

#[test]
fn renders_if_else_if_and_else() {
    let context = template_context();
    assert_eq!(
        render("{{if params.flag}}yes{{else}}no{{end}}", &context).expect("render"),
        "no"
    );
    assert_eq!(
        render(
            "{{if eq round 2}}two{{else if eq round 1}}one{{else}}other{{end}}",
            &context
        )
        .expect("render"),
        "two"
    );
    let one = json!({"round": 1});
    assert_eq!(
        render(
            "{{if eq round 2}}two{{else if eq round 1}}one{{else}}other{{end}}",
            &one
        )
        .expect("render"),
        "one"
    );
    let other = json!({"round": 9});
    assert_eq!(
        render(
            "{{if eq round 2}}two{{else if eq round 1}}one{{else}}other{{end}}",
            &other
        )
        .expect("render"),
        "other"
    );
    assert_eq!(
        render(
            "{{if true}}{{if false}}a{{else}}b{{end}}{{else}}c{{end}}",
            &context
        )
        .expect("render"),
        "b"
    );
}

#[test]
fn uses_go_like_truthiness() {
    for value in [
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
        json!(null),
    ] {
        assert_eq!(
            render("{{if .}}yes{{else}}no{{end}}", &value).expect("render"),
            "no",
            "value {value} must be falsey"
        );
    }
    for value in [
        json!(true),
        json!(1),
        json!(-1),
        json!("x"),
        json!([1]),
        json!({"a": 1}),
    ] {
        assert_eq!(
            render("{{if .}}yes{{else}}no{{end}}", &value).expect("render"),
            "yes",
            "value {value} must be truthy"
        );
    }
}

#[test]
fn renders_ranges_over_arrays_and_objects() {
    let context = template_context();
    assert_eq!(
        render("{{range steps.build.json.items}}{{ . }},{{end}}", &context).expect("render"),
        "a,b,c,"
    );
    assert_eq!(
        render("{{range .}}x{{else}}empty{{end}}", &json!([])).expect("render"),
        "empty"
    );
    assert_eq!(
        render("{{range .}}x{{else}}empty{{end}}", &json!({})).expect("render"),
        "empty"
    );
    assert_eq!(
        render("{{range .}}{{.}};{{end}}", &json!({"b": 2, "a": 1})).expect("render"),
        "1;2;"
    );
    assert_eq!(
        render(
            "{{range .}}{{.name}};{{end}}",
            &json!([{"name": "x"}, {"name": "y"}])
        )
        .expect("render"),
        "x;y;"
    );
    assert_eq!(
        render(
            "{{range steps.build.json.items}}{{ $.params.repo }}={{ . }};{{end}}",
            &context
        )
        .expect("render"),
        "acme/app=a;acme/app=b;acme/app=c;"
    );
    assert_eq!(
        render(
            "{{range .}}{{if eq . 2}}two{{else}}{{.}}{{end}}{{end}}",
            &json!([1, 2, 3])
        )
        .expect("render"),
        "1two3"
    );
}

#[test]
fn renders_with_and_restores_dot_in_else() {
    let context = template_context();
    assert_eq!(
        render("{{with params.repo}}{{.}}{{else}}none{{end}}", &context).expect("render"),
        "acme/app"
    );
    assert_eq!(
        render(
            "{{with steps.build.json}}{{.n}}{{else}}none{{end}}",
            &context
        )
        .expect("render"),
        "3"
    );
    assert_eq!(
        render(
            "{{with steps.build.json}}{{$.params.repo}}={{.n}}{{else}}none{{end}}",
            &context
        )
        .expect("render"),
        "acme/app=3"
    );
    assert_eq!(
        render("{{with .}}{{.}}{{else}}none{{end}}", &json!("")).expect("render"),
        "none"
    );
    assert_eq!(
        render("{{with .}}{{.}}{{else}}none{{end}}", &json!(false)).expect("render"),
        "none"
    );
}

#[test]
fn ignores_template_comments() {
    let context = template_context();
    assert_eq!(
        render("a{{/* ignored }} and {{ */}}b", &context).expect("render"),
        "ab"
    );
    assert_eq!(
        render("a \n {{- /* comment */ -}} \n b", &context).expect("render"),
        "ab"
    );
    assert_eq!(
        render("{{if true}}a{{/* c */}}b{{end}}", &context).expect("render"),
        "ab"
    );
}

#[test]
fn evaluates_comparison_logic_and_function_pipelines() {
    let context = template_context();
    assert_eq!(render("{{eq round 2}}", &context).expect("render"), "true");
    assert_eq!(
        render("{{eq round 2 3}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{eq round 4 3}}", &context).expect("render"),
        "false"
    );
    assert_eq!(render("{{ne round 3}}", &context).expect("render"), "true");
    assert_eq!(render("{{lt 1 2}}", &context).expect("render"), "true");
    assert_eq!(render("{{le 2 2}}", &context).expect("render"), "true");
    assert_eq!(render("{{gt 3 2}}", &context).expect("render"), "true");
    assert_eq!(render("{{ge 2 2}}", &context).expect("render"), "true");
    assert_eq!(
        render("{{lt \"a\" \"b\"}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{and true 1 \"x\"}}", &context).expect("render"),
        "x"
    );
    assert_eq!(
        render("{{and true 0 \"x\"}}", &context).expect("render"),
        "0"
    );
    assert_eq!(
        render("{{or false 0 \"x\"}}", &context).expect("render"),
        "x"
    );
    assert_eq!(render("{{or false 0}}", &context).expect("render"), "0");
    assert_eq!(render("{{not false}}", &context).expect("render"), "true");
    assert_eq!(
        render("{{not (eq round 1)}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render(
            "{{and (eq round 2) (eq params.repo \"acme/app\")}}",
            &context
        )
        .expect("render"),
        "true"
    );
    assert_eq!(
        render("{{len steps.build.json.items}}", &context).expect("render"),
        "3"
    );
    assert_eq!(
        render("{{len params.repo}}", &context).expect("render"),
        "8"
    );
    assert_eq!(
        render("{{index steps.build.json.items 1}}", &context).expect("render"),
        "b"
    );
    assert_eq!(
        render("{{index steps.build.json \"n\"}}", &context).expect("render"),
        "3"
    );
    assert_eq!(render("{{print 1 2}}", &context).expect("render"), "1 2");
    assert_eq!(
        render("{{print \"a\" 1 2}}", &context).expect("render"),
        "a1 2"
    );
    assert_eq!(
        render("{{printf \"%s=%d\" \"n\" 3}}", &context).expect("render"),
        "n=3"
    );
    assert_eq!(
        render("{{printf \"%03d\" 7}}", &context).expect("render"),
        "007"
    );
    assert_eq!(
        render("{{printf \"%.2f\" 1.234}}", &context).expect("render"),
        "1.23"
    );
    assert_eq!(
        render("{{printf \"%% %v\" true}}", &context).expect("render"),
        "% true"
    );
    assert_eq!(
        render("{{printf \"%q\" \"hi\"}}", &context).expect("render"),
        "\"hi\""
    );
}

#[test]
fn pipes_values_into_functions() {
    let context = template_context();
    assert_eq!(
        render("{{params.repo | printf \"repo=%s\"}}", &context).expect("render"),
        "repo=acme/app"
    );
    assert_eq!(
        render("{{steps.build.stdout | len}}", &context).expect("render"),
        "8"
    );
    assert_eq!(
        render("{{ \"hi\" | print \"x\" }}", &context).expect("render"),
        "xhi"
    );
    assert_eq!(
        render("{{steps.build.exitCode | eq 0}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{params.repo | printf \"%s/%s\" \"org\"}}", &context).expect("render"),
        "org/acme/app"
    );
}

#[test]
fn and_or_short_circuit() {
    let context = template_context();
    assert_eq!(
        render("{{and false .missing.path}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{or true .missing.path}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{and true false .missing.path}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{or false true .missing.path}}", &context).expect("render"),
        "true"
    );
}

#[test]
fn renders_literal_numbers_strings_and_nested_indices() {
    let context = template_context();
    assert_eq!(render("{{1}}", &context).expect("render"), "1");
    assert_eq!(render("{{-1}}", &context).expect("render"), "-1");
    assert_eq!(render("{{1.5}}", &context).expect("render"), "1.5");
    assert_eq!(render("{{1e3}}", &context).expect("render"), "1000.0");
    assert_eq!(
        render("{{ \"a\\tb\\n\" }}", &context).expect("render"),
        "a\tb\n"
    );
    assert_eq!(
        render("{{index . 0 1}}", &json!([[1, 2], [3, 4]])).expect("render"),
        "2"
    );
    assert_eq!(
        render("{{printf \"%t %x %o %b %v\" true 15 8 5 2.5}}", &context).expect("render"),
        "true f 10 101 2.5"
    );
}

#[test]
fn trims_whitespace_with_markers() {
    let context = json!({"name": "x"});
    assert_eq!(
        render("a  \n\t{{- .name }}", &context).expect("render"),
        "ax"
    );
    assert_eq!(
        render("{{ .name -}}  \n\t b", &context).expect("render"),
        "xb"
    );
    assert_eq!(
        render("a \n {{- .name -}} \n b", &context).expect("render"),
        "axb"
    );
    assert_eq!(
        render("{{- if true -}} a {{- end -}}", &context).expect("render"),
        "a"
    );
    assert_eq!(
        render("start\n{{- if true }}\nmid\n{{- end }}\nend", &context).expect("render"),
        "start\nmid\nend"
    );
}

#[test]
fn renders_literal_braces() {
    let context = template_context();
    assert_eq!(render("{{ \"{{\" }}", &context).expect("render"), "{{");
    assert_eq!(render("{{ \"}}\" }}", &context).expect("render"), "}}");
    assert_eq!(render("{{\"{{\"}}", &context).expect("render"), "{{");
    assert_eq!(render("{{\"}}\"}}", &context).expect("render"), "}}");
    assert_eq!(
        render("{{`{{ .x }}`}}", &context).expect("render"),
        "{{ .x }}"
    );
    assert_eq!(
        render("x{{ \"{{\" }}if y{{ \"}}\" }}z", &context).expect("render"),
        "x{{if y}}z"
    );
}

#[test]
fn stringifies_plain_template_values() {
    let context = template_context();
    assert_eq!(render("{{params.empty}}", &context).expect("render"), "");
    assert_eq!(
        render("{{params.flag}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{steps.build.score}}", &context).expect("render"),
        "0.75"
    );
    assert_eq!(
        render("{{steps.build.json.items}}", &context).expect("render"),
        "[\"a\",\"b\",\"c\"]"
    );
    assert_eq!(
        render("{{steps.build.json}}", &context).expect("render"),
        r#"{"n":3,"ok":true,"items":["a","b","c"]}"#
    );
}

#[test]
fn reports_template_syntax_and_execution_errors() {
    let cases = [
        ("{{ params.repo", "unterminated"),
        ("{{ }}", "empty template action"),
        ("{{ \"unterminated }}", "unterminated quoted string"),
        ("{{/* unclosed }}", "unterminated template comment"),
        ("{{ missing.path }}", "unknown template reference"),
        ("{{ .missing }}", "unknown template reference"),
        ("{{ $missing }}", "unsupported template variable"),
        ("{{ .foo. }}", "invalid template path"),
        ("{{ foo 1 }}", "unknown template function"),
        ("{{ 1 2 }}", "unexpected argument"),
        ("{{ 1 | }}", "expected a command after `|`"),
        ("{{ (1 }}", "missing `)`"),
        ("{{ end }}", "unexpected `{{end}}`"),
        ("{{ else }}", "unexpected `{{else}}`"),
        ("{{ if true }}x", "missing `{{end}}`"),
        ("{{ if }}x{{end}}", "`if` requires a pipeline"),
        (
            "{{ range . }}x{{ else if true }}y{{ end }}",
            "does not support `else if`",
        ),
        (
            "{{ range params.repo }}x{{ end }}",
            "cannot range over string",
        ),
        ("{{ eq 1 }}", "expects at least 2"),
        ("{{ len 1 2 }}", "expects 1 argument"),
        ("{{ index }}", "expects at least 1"),
        (
            "{{ index steps.build.json.items 9 }}",
            "array index 9 out of range",
        ),
        (
            "{{ index steps.build.json.items \"x\" }}",
            "index must be an integer",
        ),
        ("{{ index steps.build.stdout 0 }}", "cannot index into"),
        ("{{ lt 1 \"a\" }}", "cannot compare"),
        ("{{ printf \"%d\" }}", "not enough arguments"),
        ("{{ printf \"%d\" 1 2 }}", "unused argument"),
        ("{{ printf \"%z\" 1 }}", "unsupported verb"),
        ("{{ 1 | . }}", "cannot pipe a value"),
        ("{{ .steps.build.stdout.extra }}", "cannot descend into"),
        ("{{ steps.build.json.items.5 }}", "array index out of range"),
        ("{{ steps.build.json.items.x }}", "not a valid array index"),
    ];
    for (template, expected) in cases {
        let message = template_error(template);
        assert!(
            message.contains(expected),
            "template {template:?} should fail with {expected:?}, got {message:?}"
        );
    }
}

#[test]
fn reports_template_error_locations() {
    let message = template_error("line one\n{{ missing.path }}");
    assert!(message.contains("line 2"), "message: {message}");
    let message = template_error("{{/* unclosed }}");
    assert!(message.contains("line 1"), "message: {message}");
}

#[test]
fn printf_supports_go_style_flags_precision_and_verbs() {
    let context = template_context();
    assert_eq!(
        render("{{printf \"%.3d\" 7}}", &context).expect("render"),
        "007"
    );
    assert_eq!(
        render("{{printf \"%05d\" -5}}", &context).expect("render"),
        "-0005"
    );
    assert_eq!(
        render("{{printf \"%+d\" 5}}", &context).expect("render"),
        "+5"
    );
    assert_eq!(
        render("{{printf \"% d\" 5}}", &context).expect("render"),
        " 5"
    );
    assert_eq!(
        render("{{printf \"%#x\" 42}}", &context).expect("render"),
        "0x2a"
    );
    assert_eq!(
        render("{{printf \"%#08x\" 42}}", &context).expect("render"),
        "0x0000002a"
    );
    assert_eq!(
        render("{{printf \"%#o\" 8}}", &context).expect("render"),
        "010"
    );
    assert_eq!(
        render("{{printf \"%#b\" 5}}", &context).expect("render"),
        "0b101"
    );
    assert_eq!(
        render("{{printf \"%e\" 1234.5}}", &context).expect("render"),
        "1.234500e+03"
    );
    assert_eq!(
        render("{{printf \"%g\" 1.5}}", &context).expect("render"),
        "1.5"
    );
    assert_eq!(
        render("{{printf \"%.3g\" 1234}}", &context).expect("render"),
        "1.23e+03"
    );
    assert_eq!(
        render("{{printf \"%.0f\" 1.6}}", &context).expect("render"),
        "2"
    );
    assert_eq!(
        render("{{printf \"%q\" \"a\\tb\"}}", &context).expect("render"),
        "\"a\\tb\""
    );
    assert_eq!(
        render("{{printf \"%*d\" 5 7}}", &context).expect("render"),
        "    7"
    );
    assert_eq!(
        render("{{printf \"%.*f\" 2 1.234}}", &context).expect("render"),
        "1.23"
    );
}

#[test]
fn postfix_access_only_binds_when_adjacent() {
    let context = json!({
        "a": 1,
        "b": true,
        "items": [{"name": "x"}],
        "x": "root"
    });
    assert_eq!(
        render("{{if and (eq .a 1) .b}}yes{{else}}no{{end}}", &context).expect("render"),
        "yes"
    );
    assert_eq!(
        render("{{printf \"%s-%s\" (index .items 0).name .x}}", &context).expect("render"),
        "x-root"
    );
    assert_eq!(
        render("{{(index .items 0).name}}", &context).expect("render"),
        "x"
    );
    assert_eq!(
        render("{{and (eq .a 1) (index .items 0).name}}", &context).expect("render"),
        "x"
    );
}

#[test]
fn printf_preserves_arbitrary_precision_integers() {
    let context: serde_json::Value =
        serde_json::from_str(r#"{"big": 123456789012345678901234567890}"#).expect("json");
    assert_eq!(
        render("{{printf \"%d\" .big}}", &context).expect("render"),
        "123456789012345678901234567890"
    );
    assert_eq!(
        render("{{printf \"%x\" .big}}", &context).expect("render"),
        "18ee90ff6c373e0ee4e3f0ad2"
    );
    assert_eq!(
        render("{{printf \"%o\" .big}}", &context).expect("render"),
        "143564417755415637016711617605322"
    );
    assert_eq!(
        render("{{printf \"%d\" 123456789012345678901234567890}}", &context).expect("render"),
        "123456789012345678901234567890"
    );
    assert_eq!(
        render("{{printf \"%x\" 18446744073709551615}}", &context).expect("render"),
        "ffffffffffffffff"
    );
}

#[test]
fn index_rejects_fractional_boolean_and_out_of_range_keys() {
    let context = json!({"items": ["a", "b"]});
    assert_eq!(
        render("{{index .items 1.0}}", &context).expect("render"),
        "b"
    );
    for template in [
        "{{index .items 1.9}}",
        "{{index .items true}}",
        "{{index .items -1}}",
        "{{index .items 99}}",
    ] {
        let message = render(template, &context)
            .expect_err("invalid index must fail")
            .to_string();
        assert!(
            message.contains("index"),
            "template {template:?} produced {message:?}"
        );
    }
}

#[test]
fn printf_general_format_uses_shortest_digits() {
    let context = template_context();
    assert_eq!(
        render("{{printf \"%g\" 100}}", &context).expect("render"),
        "100"
    );
    assert_eq!(
        render("{{printf \"%g\" 1000000}}", &context).expect("render"),
        "1e+06"
    );
    assert_eq!(
        render("{{printf \"%g\" 1234567}}", &context).expect("render"),
        "1.234567e+06"
    );
    assert_eq!(
        render("{{printf \"%g\" 0.0001}}", &context).expect("render"),
        "0.0001"
    );
    assert_eq!(
        render("{{printf \"%g\" 0.00001}}", &context).expect("render"),
        "1e-05"
    );
    assert_eq!(
        render("{{printf \"%g\" 1.2345678901234567}}", &context).expect("render"),
        "1.2345678901234567"
    );
    assert_eq!(
        render("{{printf \"%#g\" 0}}", &context).expect("render"),
        "0.00000"
    );
    assert_eq!(
        render("{{printf \"%#g\" 1.5}}", &context).expect("render"),
        "1.50000"
    );
    assert_eq!(
        render("{{printf \"%#g\" 100}}", &context).expect("render"),
        "100.000"
    );
}

#[test]
fn printf_general_explicit_precision_uses_rounded_exponent() {
    let context = template_context();
    for (template, expected) in [
        ("{{printf \"%.6g\" 999999.9}}", "1e+06"),
        ("{{printf \"%.6g\" 999999.5}}", "1e+06"),
        ("{{printf \"%.3g\" 999.9}}", "1e+03"),
        ("{{printf \"%.2g\" 99.9}}", "1e+02"),
        ("{{printf \"%.1g\" 9.9}}", "1e+01"),
        ("{{printf \"%.4g\" 9999.5}}", "1e+04"),
        ("{{printf \"%.6g\" 0.00009999999}}", "0.0001"),
        ("{{printf \"%#.6g\" 999999.9}}", "1.00000e+06"),
    ] {
        assert_eq!(
            render(template, &context).expect("render"),
            expected,
            "template {template}"
        );
    }
}

#[test]
fn printf_alternate_general_without_precision_matches_go() {
    let context = template_context();
    for (template, expected) in [
        ("{{printf \"%#g\" 0}}", "0.00000"),
        ("{{printf \"%#g\" 1.5}}", "1.50000"),
        ("{{printf \"%#g\" 100}}", "100.000"),
        ("{{printf \"%#g\" 999999.9}}", "999999.9"),
        ("{{printf \"%#g\" 999999.5}}", "999999.5"),
        ("{{printf \"%#g\" 1e6}}", "1.00000e+06"),
        ("{{printf \"%#g\" 1234567}}", "1.234567e+06"),
        ("{{printf \"%#g\" 0.0001}}", "0.000100000"),
        ("{{printf \"%#g\" 1e-5}}", "1.00000e-05"),
        (
            "{{printf \"%#g\" 1.2345678901234567}}",
            "1.2345678901234567",
        ),
        ("{{printf \"%#g\" 123456}}", "123456."),
        ("{{printf \"%#g\" 999999.4}}", "999999.4"),
        ("{{printf \"%#g\" 99999.9}}", "99999.9"),
        ("{{printf \"%#g\" 9.9}}", "9.90000"),
        ("{{printf \"%#g\" 0.1}}", "0.100000"),
        ("{{printf \"%#g\" 1.0}}", "1.00000"),
        ("{{printf \"%#g\" -1.5}}", "-1.50000"),
    ] {
        assert_eq!(
            render(template, &context).expect("render"),
            expected,
            "template {template}"
        );
    }
}

#[test]
fn printf_integer_zero_padding_and_alternate_forms_match_go() {
    let context = template_context();
    assert_eq!(
        render("{{printf \"%05.3d\" 7}}", &context).expect("render"),
        "  007"
    );
    assert_eq!(
        render("{{printf \"%#08x\" 42}}", &context).expect("render"),
        "0x0000002a"
    );
    assert_eq!(
        render("{{printf \"%#08b\" 5}}", &context).expect("render"),
        "0b00000101"
    );
    assert_eq!(
        render("{{printf \"%#.0f\" 2}}", &context).expect("render"),
        "2."
    );
    assert_eq!(
        render("{{printf \"%#.0e\" 2}}", &context).expect("render"),
        "2.e+00"
    );
    for template in [
        "{{printf \"%#.0o\" 0}}",
        "{{printf \"%#.0x\" 0}}",
        "{{printf \"%#.0X\" 0}}",
        "{{printf \"%#.0b\" 0}}",
    ] {
        assert_eq!(
            render(template, &context).expect("render"),
            "",
            "template {template}"
        );
    }
}

#[test]
fn compares_mixed_exponent_and_plain_big_integers_exactly() {
    let context = template_context();
    assert_eq!(
        render("{{eq 1e30 1000000000000000000000000000001}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{lt 1e30 1000000000000000000000000000001}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{gt 1000000000000000000000000000001 1e30}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{eq 1e30 1000000000000000000000000000000}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{eq 1e20 100000000000000000001}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{lt 1e20 100000000000000000001}}", &context).expect("render"),
        "true"
    );

    let big: serde_json::Value =
        serde_json::from_str(r#"{"big": 1000000000000000000000000000001}"#).expect("json");
    assert_eq!(render("{{eq .big 1e30}}", &big).expect("render"), "false");
    assert_eq!(render("{{gt .big 1e30}}", &big).expect("render"), "true");
    assert_eq!(
        render("{{lt .big 1000000000000000000000000000002}}", &big).expect("render"),
        "true"
    );

    assert_eq!(
        render("{{printf \"%d\" 1e30}}", &context).expect("render"),
        "1000000000000000000000000000000"
    );
    assert_eq!(
        render(
            "{{printf \"%d\" 1000000000000000000000000000000}}",
            &context
        )
        .expect("render"),
        "1000000000000000000000000000000"
    );
    assert_eq!(
        render("{{printf \"%x\" 1e30}}", &context).expect("render"),
        "c9f2c9cd04674edea40000000"
    );
    assert_eq!(
        render(
            "{{printf \"%x\" 1000000000000000000000000000000}}",
            &context
        )
        .expect("render"),
        "c9f2c9cd04674edea40000000"
    );
}

#[test]
fn plain_big_integer_literals_are_exact() {
    let context = template_context();
    assert_eq!(
        render(
            "{{eq 100000000000000000000 100000000000000000001}}",
            &context
        )
        .expect("render"),
        "false"
    );
    assert_eq!(
        render(
            "{{lt 100000000000000000000 100000000000000000001}}",
            &context
        )
        .expect("render"),
        "true"
    );
    assert_eq!(
        render("{{100000000000000000000}}", &context).expect("render"),
        "100000000000000000000"
    );
    assert_eq!(
        render("{{printf \"%q\" 100000000000000000000}}", &context).expect("render"),
        "'�'"
    );

    let big: serde_json::Value =
        serde_json::from_str(r#"{"big": 100000000000000000001}"#).expect("json");
    assert_eq!(
        render("{{eq .big 100000000000000000000}}", &big).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{gt .big 100000000000000000000}}", &big).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{lt .big 100000000000000000002}}", &big).expect("render"),
        "true"
    );
}

#[test]
fn printf_formats_exponent_form_numbers_exactly() {
    let context = template_context();
    assert_eq!(
        render("{{printf \"%d\" 1e30}}", &context).expect("render"),
        "1000000000000000000000000000000"
    );
    assert_eq!(
        render("{{printf \"%x\" 1e30}}", &context).expect("render"),
        "c9f2c9cd04674edea40000000"
    );
    assert_eq!(
        render("{{printf \"%d\" 9007199254740993e0}}", &context).expect("render"),
        "9007199254740993"
    );
    assert_eq!(
        render("{{printf \"%x\" 9007199254740993e0}}", &context).expect("render"),
        "20000000000001"
    );

    let exponent_context: serde_json::Value =
        serde_json::from_str(r#"{"big": 123456789012345678901234567890e0}"#).expect("json");
    assert_eq!(
        render("{{printf \"%d\" .big}}", &exponent_context).expect("render"),
        "123456789012345678901234567890"
    );
    assert_eq!(
        render("{{printf \"%x\" .big}}", &exponent_context).expect("render"),
        "18ee90ff6c373e0ee4e3f0ad2"
    );
    assert!(render("{{printf \"%d\" 1e2000000}}", &context).is_err());
}

#[test]
fn printf_quotes_runes_strings_and_unicode_like_go() {
    let context = template_context();
    assert_eq!(
        render("{{printf \"%q\" \"\\u00a0\"}}", &context).expect("render"),
        "\"\\u00a0\""
    );
    assert_eq!(
        render("{{printf \"%q\" \"\\u2028\"}}", &context).expect("render"),
        "\"\\u2028\""
    );
    assert_eq!(
        render("{{printf \"%q\" \"\\u2029\"}}", &context).expect("render"),
        "\"\\u2029\""
    );
    assert_eq!(
        render("{{printf \"%q\" \"é\"}}", &context).expect("render"),
        "\"é\""
    );
    assert_eq!(
        render("{{printf \"%q\" 65}}", &context).expect("render"),
        "'A'"
    );
    assert_eq!(
        render("{{printf \"%q\" 10}}", &context).expect("render"),
        "'\\n'"
    );
    assert!(render("{{printf \"%q\" true}}", &context).is_err());
    assert!(render("{{printf \"%q\" 1.5}}", &context).is_err());
    for (template, expected) in [
        ("{{printf \"%q\" \"\\u200e\"}}", "\"\\u200e\""),
        ("{{printf \"%q\" \"\\u2060\"}}", "\"\\u2060\""),
        ("{{printf \"%q\" \"\\u0600\"}}", "\"\\u0600\""),
        ("{{printf \"%q\" \"\\ue000\"}}", "\"\\ue000\""),
        ("{{printf \"%q\" \"\\U0010ffff\"}}", "\"\\U0010ffff\""),
        ("{{printf \"%q\" 4294967296}}", "'�'"),
        ("{{printf \"%q\" 1114112}}", "'�'"),
        ("{{printf \"%q\" 1e30}}", "'�'"),
    ] {
        assert_eq!(
            render(template, &context).expect("render"),
            expected,
            "template {template}"
        );
    }
}

#[test]
fn printf_reports_over_cap_integer_expansion() {
    let context = template_context();
    for template in [
        "{{printf \"%d\" 1e2000000}}",
        "{{printf \"%x\" 1e2000000}}",
        "{{printf \"%q\" 1e2000000}}",
    ] {
        let message = render(template, &context)
            .expect_err("over-cap expansion must fail")
            .to_string();
        assert!(
            message.contains("exceeds the maximum"),
            "template {template:?} produced {message:?}"
        );
    }
}

#[test]
fn printf_rejects_unbounded_field_sizes() {
    let context = template_context();
    for template in [
        "{{printf \"%18446744073709551615d\" 7}}",
        "{{printf \"%1000000000000d\" 7}}",
        "{{printf \"%.18446744073709551615f\" 1.5}}",
        "{{printf \"%*d\" 1000000000000 7}}",
        "{{printf \"%.*f\" 1000000000000 1.5}}",
    ] {
        let message = render(template, &context)
            .expect_err("huge field sizes must fail")
            .to_string();
        assert!(
            message.contains("exceeds the maximum"),
            "template {template:?} produced {message:?}"
        );
    }
}

#[test]
fn compares_large_integers_exactly() {
    let context = json!({
        "big": 9_007_199_254_740_993i64,
        "u": 18_446_744_073_709_551_615u64
    });
    assert_eq!(
        render("{{eq 9007199254740992 9007199254740993}}", &context).expect("render"),
        "false"
    );
    assert_eq!(
        render("{{lt 9007199254740992 9007199254740993}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{gt 9007199254740993 9007199254740992}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{eq .big 9007199254740993}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{lt .big 9007199254740994}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{gt .big 9007199254740992}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{eq .u 18446744073709551615}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render("{{gt .u 18446744073709551614}}", &context).expect("render"),
        "true"
    );
    assert_eq!(
        render(
            "{{eq 123456789012345678901234567890 123456789012345678901234567891}}",
            &context
        )
        .expect("render"),
        "false"
    );
    assert_eq!(
        render(
            "{{lt 123456789012345678901234567890 123456789012345678901234567891}}",
            &context
        )
        .expect("render"),
        "true"
    );
}

#[test]
fn parses_fractional_literals_without_a_leading_zero() {
    let array = json!([10, 20, 30]);
    assert_eq!(render("{{.5}}", &array).expect("render"), "0.5");
    assert_eq!(render("{{-.5}}", &array).expect("render"), "-0.5");
    let object = json!({"5": "five"});
    assert_eq!(render("{{.5}}", &object).expect("render"), "0.5");
    assert_eq!(render("{{index . 0}}", &array).expect("render"), "10");
}

#[test]
fn range_over_null_uses_else() {
    assert_eq!(
        render("{{range .}}x{{else}}empty{{end}}", &json!(null)).expect("render"),
        "empty"
    );
}

#[test]
fn preserves_legacy_paths_with_spaces_in_segments() {
    let context = json!({
        "params": {"my key": "spaced"},
        "steps": {"build test": {"stdout": "legacy"}}
    });
    assert_eq!(
        render("{{ params.my key }}", &context).expect("render"),
        "spaced"
    );
    assert_eq!(
        render("{{ steps.build test.stdout }}", &context).expect("render"),
        "legacy"
    );
    let message = render("{{ params.my key | len }}", &context)
        .expect_err("operators disable the legacy fallback")
        .to_string();
    assert!(
        message.contains("unexpected argument"),
        "message: {message}"
    );
}

#[test]
fn supports_postfix_access_after_parenthesized_pipelines() {
    assert_eq!(
        render("{{(index . 0).name}}", &json!([{"name": "x"}])).expect("render"),
        "x"
    );
    assert_eq!(
        render("{{(index . \"a\").b}}", &json!({"a": {"b": "y"}})).expect("render"),
        "y"
    );
    assert_eq!(
        render(
            "{{(index . 0).name | printf \"%s!\"}}",
            &json!([{"name": "x"}])
        )
        .expect("render"),
        "x!"
    );
}

#[test]
fn control_error_messages_keep_double_braces() {
    let message = template_error("{{range .items}}");
    assert!(message.contains("{{end}}"), "message: {message}");
    let message = template_error("{{with .items}}");
    assert!(message.contains("{{end}}"), "message: {message}");
    let message = template_error("{{range .items}}x{{else}}y{{else}}z{{end}}");
    assert!(message.contains("{{else}}"), "message: {message}");
    let message = template_error("{{with .items}}x{{else}}y{{else}}z{{end}}");
    assert!(message.contains("{{else}}"), "message: {message}");
}

#[test]
fn rejects_deeply_nested_templates_on_small_stacks() {
    let mut nested_ifs = String::new();
    for _ in 0..10_000 {
        nested_ifs.push_str("{{if true}}");
    }
    nested_ifs.push('x');
    for _ in 0..10_000 {
        nested_ifs.push_str("{{end}}");
    }
    let result = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || render(&nested_ifs, &json!({})))
        .expect("spawn")
        .join()
        .expect("thread must not panic");
    let message = result.expect_err("deeply nested ifs must fail").to_string();
    assert!(message.contains("nesting"), "message: {message}");

    let mut nested_parens = String::from("{{");
    nested_parens.push_str(&"(".repeat(10_000));
    nested_parens.push('1');
    nested_parens.push_str(&")".repeat(10_000));
    nested_parens.push_str("}}");
    let result = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || render(&nested_parens, &json!({})))
        .expect("spawn")
        .join()
        .expect("thread must not panic");
    let message = result
        .expect_err("deeply nested pipelines must fail")
        .to_string();
    assert!(message.contains("nesting"), "message: {message}");

    let mut else_ifs = String::from("{{if false}}");
    for _ in 0..10_000 {
        else_ifs.push_str("{{else if true}}");
    }
    else_ifs.push('x');
    else_ifs.push_str("{{end}}");
    let result = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || render(&else_ifs, &json!({})))
        .expect("spawn")
        .join()
        .expect("thread must not panic");
    let message = result
        .expect_err("deep else-if chains must fail")
        .to_string();
    assert!(message.contains("nesting"), "message: {message}");
}

#[test]
fn renders_many_actions_quickly() {
    let mut template = String::with_capacity(50_000 * 20);
    for _ in 0..50_000 {
        template.push_str("{{ params.value }}");
    }
    let context = json!({"params": {"value": "x"}});
    let started = std::time::Instant::now();
    let rendered = render(&template, &context).expect("render");
    let elapsed = started.elapsed();
    assert_eq!(rendered.len(), 50_000);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "50k actions took {elapsed:?}"
    );
}

#[test]
fn native_parameter_overrides_are_injected_before_interpolation() {
    let source = r#"
package demo
params: { name: string | *"world" @tag(name) }
workflow: {
    name: "params"
    steps: {
        a: {
            kind: "exec"
            exec: {cmd: ["sh", "-c", "echo \(params.name)"]}
        }
    }
}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("name".to_owned(), "ostinato".to_owned())]),
    )
    .expect("compile");
    let cmd = &compiled
        .spec
        .step("a")
        .expect("a")
        .exec
        .as_ref()
        .expect("exec")
        .cmd;
    assert_eq!(cmd[2], "echo ostinato");
}

#[test]
fn native_skips_attributes_with_equals_and_omits_optional_fields() {
    let source = r#"
package demo
params: { n: int | *1 @tag(n, type=int) }
workflow: {
    name: "attrs"
    steps: {
        a: {
            kind: "noop"
            optional?: "omitted"
        }
    }
}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("n".to_owned(), "7".to_owned())]),
    )
    .expect("compile");
    let step = compiled.spec.step("a").expect("a");
    assert_eq!(step.kind, ostinato_domain::StepKind::Noop);
    assert_eq!(
        compiled.resolve_params(&BTreeMap::new()).expect("params")["n"],
        7
    );
}

#[test]
fn native_defaults_work_in_multibranch_disjunctions() {
    let file = parse("package p\nx: *1 | 2 | 3\n").expect("parse");
    let value = eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"], 1);
}

#[test]
fn unknown_workflow_fields_are_rejected() {
    let source = r#"
package demo
workflow: { maxparallel: 2, steps: { a: { kind: "noop" } } }
"#;
    let error = compile(source).expect_err("must reject typo");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn invalid_until_is_rejected_at_validation_time() {
    let source = r#"
package demo
workflow: {
    steps: {
        a: { kind: "noop", needs: ["a"], cycle: {max: 2, until: "steps.missing.score >= 0.9"} }
    }
}
"#;
    let error = compile(source).expect_err("must reject unknown reference");
    assert!(error.to_string().contains("until is invalid"));
}

#[test]
fn until_accepts_nested_json_provider_and_hyphenated_steps() {
    let source = r#"
package demo
workflow: {
    steps: {
        review: {
            kind: "exec"
            needs: ["refine"]
            cycle: {max: 3, until: "steps.review.json.confidence >= 0.9"}
            exec: {cmd: ["sh", "-c", "echo '{\"confidence\": 0.5}'"]}
        }
        refine: {kind: "noop", needs: ["review"]}
        "job-one": {
            kind: "noop"
            needs: ["job-one"]
            cycle: {max: 2, until: "steps.job-one.score >= 0.5"}
        }
    }
}
"#;
    ostinato_lang::compile(source).expect("nested/hyphenated until must validate");
}

#[test]
fn step_level_unknown_fields_are_rejected() {
    let source = r#"
package demo
workflow: { steps: { a: { kind: "noop", timeoutMS: 1 } } }
"#;
    let error = ostinato_lang::compile(source).expect_err("must reject step typo");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn retry_int_shorthand_respects_the_cap() {
    let source = r#"
package demo
workflow: { steps: { a: { kind: "noop", retry: 101 } } }
"#;
    let error = ostinato_lang::compile(source).expect_err("must reject large retry");
    assert!(error.to_string().contains("<= 100"));
}

#[test]
fn native_overrides_apply_before_top_level_comprehensions() {
    let source = r#"
package demo
params: { env: string | *"dev" }
base: { steps: { a: { kind: "noop" } } }
if params.env == "prod" {
    base: { steps: { b: { kind: "noop" } } }
}
workflow: base
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("env".to_owned(), "prod".to_owned())]),
    )
    .expect("compile");
    assert_eq!(compiled.spec.steps.len(), 2);
    assert!(compiled.spec.step("b").is_some());
}

#[test]
fn parameter_overrides_are_constraints_and_dependent_fields_see_them() {
    let source = r#"
package demo
params: {
    name: string @tag(name)
    greeting: "Hello \(name)"
}
workflow: {
    name: "params"
    steps: {
        a: {kind: "exec", exec: {cmd: ["echo", params.greeting]}}
    }
}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("compile");
    let cmd = &compiled
        .spec
        .step("a")
        .expect("a")
        .exec
        .as_ref()
        .expect("exec")
        .cmd;
    assert_eq!(cmd[1], "Hello Alice");
}

#[test]
fn parameter_overrides_respect_fixed_values_and_types() {
    let fixed = r#"
package demo
params: {x: "fixed"}
workflow: {steps: {a: {kind: "noop"}}}
"#;
    assert!(
        ostinato_lang::compile_with_params(
            fixed,
            &BTreeMap::from([("x".to_owned(), "other".to_owned())])
        )
        .is_err()
    );

    let typed = r#"
package demo
params: {n: int}
workflow: {steps: {a: {kind: "noop"}}}
"#;
    assert!(
        ostinato_lang::compile_with_params(
            typed,
            &BTreeMap::from([("n".to_owned(), "oops".to_owned())])
        )
        .is_err()
    );
}

#[test]
fn parameter_overrides_narrow_a_default_free_disjunction() {
    let source = r#"
package demo
params: {env: "dev" | "prod"}
workflow: {steps: {a: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("env".to_owned(), "prod".to_owned())]),
    )
    .expect("compile");
    assert_eq!(
        compiled.resolve_params(&BTreeMap::new()).expect("params")["env"],
        "prod"
    );
}

#[test]
fn forward_let_references_are_lazy() {
    let source = r#"
package demo
let a = b
let b = "forward"
workflow: {name: a, steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(source).expect("compile");
    assert_eq!(compiled.spec.name, "forward");
}

#[test]
fn open_list_element_types_are_enforced() {
    let source = r#"
package demo
#L: [...string]
a: #L & [1, 2]
workflow: {steps: {s: {kind: "noop"}}}
"#;
    assert!(ostinato_lang::compile(source).is_err());
}

#[test]
fn missing_field_separators_are_rejected() {
    let source = r#"
package demo
workflow: {name: "p" steps: {a: {kind: "noop"}}}
"#;
    let error = ostinato_lang::compile(source).expect_err("missing separator");
    assert!(error.to_string().contains("expected") || error.to_string().contains("missing"));
}

#[test]
fn parameters_can_be_backed_by_a_definition_or_split_struct() {
    let source = r#"
package demo
#P: {name: string @tag(name), ...}
params: #P & {greeting: "Hello \(name)"}
workflow: {
    name: "\(params.greeting)"
    steps: {s: {kind: "noop"}}
}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("compile");
    assert_eq!(compiled.spec.name, "Hello Alice");
}

#[test]
fn dependent_parameter_overrides_are_order_independent() {
    let source = r#"
package demo
params: {
    a: "A-\(z)" @tag(a)
    z: string @tag(z)
}
workflow: {name: "\(params.a)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([
            ("a".to_owned(), "A-zzz".to_owned()),
            ("z".to_owned(), "zzz".to_owned()),
        ]),
    )
    .expect("compile");
    assert_eq!(compiled.spec.name, "A-zzz");
}

#[test]
fn open_list_prefix_unifies_with_a_concrete_list() {
    let source = r#"
package demo
#Needs: [string, ...string]
workflow: {
    steps: {
        a: {kind: "noop"}
        b: {kind: "noop", needs: #Needs & ["a"]}
    }
}
"#;
    let compiled = ostinato_lang::compile(source).expect("prefix open list");
    assert_eq!(compiled.spec.step("b").expect("b").needs, vec!["a"]);
}

#[test]
fn unicode_string_builtins_match_go_semantics() {
    let source = r#"
package demo
import "strings"
let n = len("é")
let u = strings.ToUpper("ß")
workflow: {name: "\(n)-\(u)", steps: {s: {kind: "noop"}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "strings".to_owned(),
        ostinato_lang::builtin_module("strings").expect("strings builtin"),
    );
    let root =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let spec = ostinato_lang::compile_value(&root).expect("compile");
    assert_eq!(spec.name, "2-ß");
}

#[test]
fn versioned_imports_use_the_unversioned_alias() {
    let file = ostinato_lang::parse(
        "package p\nimport \"example.com/foo/lib@v0\"\nworkflow: {steps: {s: {kind: \"noop\"}}}\n",
    )
    .expect("parse");
    assert_eq!(file.imports[0].alias, "lib");
}

#[test]
fn comprehension_clause_sequences_are_supported() {
    let source = r#"
package demo
workflow: {
    steps: {
        for _, job in ["a", "b"] if job != "b" {
            "job-\(job)": {kind: "noop"}
        }
        for _, x in ["1"] for _, y in ["x", "y"] {
            "\(x)-\(y)": {kind: "noop"}
        }
    }
}
"#;
    let compiled = ostinato_lang::compile(source).expect("clause sequences");
    assert!(compiled.spec.step("job-a").is_some());
    assert!(compiled.spec.step("job-b").is_none());
    assert!(compiled.spec.step("1-x").is_some());
    assert!(compiled.spec.step("1-y").is_some());
}

#[test]
fn list_comprehensions_are_supported() {
    let source = r#"
package demo
let doubled = [for i in [1, 2] {i * 2}]
workflow: {name: "n-\(len(doubled))", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(source).expect("list comp");
    assert_eq!(compiled.spec.name, "n-2");
}

#[test]
fn parenthesized_dynamic_labels_and_if_clause_sequences_work() {
    let source = r#"
package demo
let name = "a"
workflow: {
    steps: {
        (name): {kind: "noop"}
        if true for _, x in [1, 2] { "x-\(x)": {kind: "noop"} }
    }
}
"#;
    let compiled = ostinato_lang::compile(source).expect("compile");
    assert!(compiled.spec.step("a").is_some());
    assert!(compiled.spec.step("x-1").is_some());
    assert!(compiled.spec.step("x-2").is_some());
}

#[test]
fn definitions_preserve_optional_constraints_while_open() {
    let bad = r#"
package demo
#D: {name?: string}
params: #D & {name: 1}
workflow: {name: "x", steps: {a: {kind: "noop"}}}
"#;
    assert!(ostinato_lang::compile(bad).is_err());

    let open = r#"
package demo
#W: {...}
workflow: #W & {name: "ok", steps: {a: {kind: "noop"}}}
"#;
    assert!(ostinato_lang::compile(open).is_ok());
}

#[test]
fn optional_definition_fields_are_not_materialized() {
    let source = r#"
package demo
#D: {a: 1, b?: 2}
x: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(
        value.to_json().expect("json")["x"],
        serde_json::json!({"a": 1})
    );

    let source = r#"
package demo
#D: {a: 1, b?: int}
x: #D & {b: 3}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["b"], 3);
}

#[test]
fn definition_optional_conflicts_error_only_when_materialized() {
    let source = r#"
package demo
#D: {b?: int, b?: string}
x: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"], serde_json::json!({}));

    let bad = r#"
package demo
#D: {b?: int, b?: string}
x: #D & {b: 1}
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn definition_optional_constraint_applies_to_required_fields_in_both_orders() {
    let left = r#"
package demo
#A: {v?: 1}
#B: {v: int}
x: #A & #B
x: {v: 2}
"#;
    let file = ostinato_lang::parse(left).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let right = r#"
package demo
#A: {v: int}
#B: {v?: 1}
x: #A & #B
x: {v: 2}
"#;
    let file = ostinato_lang::parse(right).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let ok = r#"
package demo
#A: {v?: int}
#B: {v: int}
x: #A & #B
x: {v: 2}
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["v"], 2);
}

#[test]
fn nested_definitions_are_not_exported_as_fields() {
    let workflow = r#"
package demo
#W: {
    #Step: {kind: "noop"}
    name: "nested"
    steps: {s: #Step}
}
workflow: #W
"#;
    let compiled = ostinato_lang::compile(workflow).expect("nested definition");
    assert_eq!(compiled.spec.name, "nested");

    let params = r#"
package demo
#P: {#Helper: "h", a: 1}
params: #P
workflow: {name: "p-\(params.a)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(params).expect("params definition");
    assert_eq!(
        compiled.resolve_params(&BTreeMap::new()).expect("params")["a"],
        1
    );
    assert!(
        compiled
            .resolve_params(&BTreeMap::new())
            .expect("params")
            .get("#Helper")
            .is_none()
    );
}

#[test]
fn definitions_behave_like_structs_in_comprehensions_and_equality() {
    let source = r#"
package demo
#Labels: {a: "x", b: "y"}
#P: {a: 1, b: 2}
workflow: {
    name: "iter-\(params == {a: 1, b: 2})"
    steps: {
        for k, v in #Labels {
            "\(k)": {kind: "noop"}
        }
        for k, v in #P {
            "p-\(k)": {kind: "noop"}
        }
    }
}
params: #P
"#;
    let compiled = ostinato_lang::compile(source).expect("definition comprehension");
    assert!(compiled.spec.step("a").is_some());
    assert!(compiled.spec.step("b").is_some());
    assert!(compiled.spec.step("p-a").is_some());
    assert!(compiled.spec.step("p-b").is_some());
    assert_eq!(compiled.spec.name, "iter-true");
}

#[test]
fn nested_definitions_are_selectable() {
    let source = r#"
package demo
#Library: {
    #Step: {kind: "noop"}
}
workflow: {
    name: "nested"
    steps: {s: #Library.#Step}
}
"#;
    let compiled = ostinato_lang::compile(source).expect("nested selector");
    assert_eq!(
        compiled.spec.step("s").expect("s").kind,
        ostinato_domain::StepKind::Noop
    );
}

#[test]
fn quoted_hash_labels_are_regular_fields() {
    let source = r##"
package demo
#P: {"#h": 9, a: 2}
x: #P
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["#h"], 9);
    assert_eq!(json["x"]["a"], 2);
}

#[test]
fn definitions_can_be_used_in_conditions_and_selectors() {
    let source = r#"
package demo
#P: {#Flag: true}
workflow: {
    if #P.#Flag {
        name: "yes"
    }
    steps: {s: {kind: "noop"}}
}
"#;
    let compiled = ostinato_lang::compile(source).expect("definition condition");
    assert_eq!(compiled.spec.name, "yes");
}

#[test]
fn recursive_nested_definitions_do_not_overflow() {
    let source = r#"
package demo
#W: {
    #Node: {
        value: int
        next?: #Node
    }
    name: "recursive"
    steps: {s: {kind: "noop"}}
}
workflow: #W
"#;
    let compiled = ostinato_lang::compile(source).expect("recursive definition");
    assert_eq!(compiled.spec.name, "recursive");
    assert!(compiled.spec.step("s").is_some());
}

#[test]
fn optional_nested_definitions_are_not_selectable() {
    let source = r#"
package demo
#D: {
    #X?: {kind: "noop"}
    a: 1
}
workflow: {
    name: "optional"
    steps: {s: #D.#X}
}
"#;
    let error = ostinato_lang::compile(source).expect_err("optional definition");
    assert!(error.to_string().contains("not found"));
}

#[test]
fn optional_fields_cannot_be_referenced_by_bare_name() {
    let field = r#"
package demo
#D: {a?: 1, y: a}
x: #D
"#;
    let file = ostinato_lang::parse(field).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("optional field reference");
    assert!(
        error
            .to_string()
            .contains("cannot reference optional field")
    );

    let definition = r#"
package demo
#D: {#X?: {a: 1}, y: #X}
x: #D
"#;
    let file = ostinato_lang::parse(definition).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("optional definition reference");
    assert!(
        error
            .to_string()
            .contains("cannot reference optional field")
    );
}

#[test]
fn optional_fields_cannot_be_selected_from_a_definition() {
    let source = r#"
package demo
#D: {a?: 1}
workflow: {name: "x", steps: {s: #D.a}}
"#;
    let error = ostinato_lang::compile(source).expect_err("optional selector");
    assert!(
        error
            .to_string()
            .contains("cannot reference optional field")
    );
}

#[test]
fn named_definition_flattens_optional_field_materialization() {
    let source = r#"
package demo
#P: {
    name?: string
    greeting: "Hello \(name)"
}
#Q: #P & {name: "Alice"}
params: #Q
workflow: {name: "\(params.greeting)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(source).expect("named flattening");
    assert_eq!(compiled.spec.name, "Hello Alice");
}

#[test]
fn generated_optional_constraint_applies_to_a_required_field() {
    let source = r#"
package demo
#W: {
    name: "real-name"
    steps: {s: {kind: "noop"}}
    for k in ["name"] {
        "\(k)"?: string
    }
}
workflow: #W
"#;
    let compiled = ostinato_lang::compile(source).expect("compatible generated optional");
    assert_eq!(compiled.spec.name, "real-name");
}

#[test]
fn generated_optional_constraint_conflicts_with_a_required_field() {
    let source = r#"
package demo
#W: {
    name: "real-name"
    steps: {s: {kind: "noop"}}
    for k in ["name"] {
        "\(k)"?: int
    }
}
workflow: #W
"#;
    assert!(ostinato_lang::compile(source).is_err());
}

#[test]
fn generated_only_optional_fields_are_not_materialized() {
    let source = r#"
package demo
#D: {
    a: 1
    if true { b?: int }
}
params: #D
workflow: {name: "len-\(len(params))", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(source).expect("generated optional");
    assert_eq!(compiled.spec.name, "len-1");
}

#[test]
fn generated_optional_conflicts_when_required_field_comes_later() {
    let source = r#"
package demo
#W: {
    steps: {s: {kind: "noop"}}
    for k in ["name"] { "\(k)"?: int }
    if true { name: "gen" }
}
workflow: #W
"#;
    assert!(ostinato_lang::compile(source).is_err());
}

#[test]
fn generated_optional_unifies_with_required_dynamic_label() {
    let direct = r#"
package demo
#W: {
    let k = "sub"
    for x in ["sub"] {
        "\(x)"?: {a: int}
        "\(k)": {a: 1}
    }
}
workflow: {name: "\(#W.sub.a)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(direct).expect("direct dynamic label");
    assert_eq!(compiled.spec.name, "1");

    let nested = r#"
package demo
#W: {
    let k = "sub"
    for x in ["sub"] {
        if true { "\(x)"?: {a: int} }
        "\(k)": {a: 1}
    }
}
workflow: {name: "\(#W.sub.a)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(nested).expect("nested dynamic label");
    assert_eq!(compiled.spec.name, "1");
}

#[test]
fn generated_optional_conflicts_with_required_dynamic_label() {
    let source = r#"
package demo
#W: {
    for x in ["sub"] {
        "\(x)"?: {a: string}
        "sub": {a: 1}
    }
}
workflow: #W
"#;
    assert!(ostinato_lang::compile(source).is_err());
}

#[test]
fn disjunction_defaults_narrow_and_export() {
    let source = r#"
package demo
a: *1 | 2
a: 2
b: int | *3
b: 4
c: (*"x" | "y") & ("x" | "y")
d: *1 | 2
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 2);
    assert_eq!(json["b"], 4);
    assert_eq!(json["c"], "x");
    assert_eq!(json["d"], 1);
}

#[test]
fn unresolved_and_ambiguous_disjunctions_are_not_concrete() {
    let unresolved = r#"
package demo
a: 1 | 2
"#;
    let file = ostinato_lang::parse(unresolved).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(value.to_json().is_none());

    let ambiguous = r#"
package demo
a: *1 | *2
"#;
    let file = ostinato_lang::parse(ambiguous).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(value.to_json().is_none());
}

#[test]
fn scalar_unification_is_symmetric_and_strict() {
    let ok = r#"
package demo
a: 1 & 1
b: 1.5 & 1.5
c: "x" & "x"
d: true & true
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["a"], 1);

    for expression in ["1 & 2", "1.5 & 2.5", "\"x\" & \"y\"", "true & false"] {
        let source = format!("package demo\na: {expression}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected conflict for {expression}"
        );
    }
}

#[test]
fn duplicate_disjunction_defaults_collapse() {
    let source = r#"
package demo
a: *1 | *1
b: 1 | 1
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["b"], 1);
}

#[test]
fn interpolation_rejects_incomplete_disjunctions() {
    let source = r#"
package demo
a: "x-\(1 | 2)"
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("incomplete interpolation");
    assert!(error.to_string().contains("non-concrete value"));
}

#[test]
fn arithmetic_uses_a_disjunction_default() {
    let source = r#"
package demo
a: (*1 | 2) + 1
b: -(*3 | 4)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 2);
    assert_eq!(json["b"], -3);
}

#[test]
fn disjunction_intersection_and_narrowing_match_cue() {
    let source = r#"
package demo
a: (*1 | 2) & 2
b: (int | *1) & 3
c: (*1 | 2) & (*2 | 3)
d: (*1 | 2) & (*1 | 3)
e: (int | string) & 1
f: (*1 | 2) & number
g: (number | *1) & 2.5
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 2);
    assert_eq!(json["b"], 3);
    assert_eq!(json["c"], 2);
    assert_eq!(json["d"], 1);
    assert_eq!(json["e"], 1);
    assert_eq!(json["f"], 1);
    assert_eq!(json["g"], 2.5);
}

#[test]
fn disjunction_drops_conflicting_alternatives_but_not_bad_references() {
    let source = r#"
package demo
a: (1 & 2) | 3
b: *1 | (2 & 3)
c: (int & string) | 1
d: (1 / 0) | 3
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    eprintln!("DEBUG {value:?}");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 3);
    assert_eq!(json["b"], 1);
    assert_eq!(json["c"], 1);
    assert_eq!(json["d"], 3);

    let bad = r#"
package demo
a: missing | 3
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn nested_disjunction_defaults_respect_direct_precedence() {
    let source = r#"
package demo
a: (*1 | 2) | *3
b: *1 | (2 | *3)
x: *1 | 2
c: x | *3
d: (x | *3) + 1
y: *1 | 2
e: *y | 3
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    eprintln!("DEBUG {value:?}");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 3);
    assert_eq!(json["b"], 1);
    assert_eq!(json["c"], 3);
    assert_eq!(json["d"], 4);
    assert_eq!(json["e"], 1);

    let incomplete = r#"
package demo
f: (*1 | (2 | *3)) & (2 | 3)
"#;
    let file = ostinato_lang::parse(incomplete).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(value.to_json().is_none());
}

#[test]
fn top_in_disjunctions_is_preserved() {
    let source = r#"
package demo
a: _ | 1
b: (_ | 1) & 2
c: (_ | *1) & 2
d: _ | _
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json();
    assert!(
        json.is_none(),
        "a/d are incomplete and b/c cannot be exported with a"
    );
}

#[test]
fn int_and_float_do_not_unify() {
    for expression in ["1 & 1.0", "(int | *1) & 1.0", "(*1 | 2) & 2.0"] {
        let source = format!("package demo\na: {expression}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected conflict for {expression}"
        );
    }
    let ok = r#"
package demo
a: (int | *1.0) & 1
b: number & 1.0
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["b"], 1.0);
}

#[test]
fn unresolved_disjunctions_are_rejected_in_boolean_contexts() {
    for expression in [
        "(1 | 2) == 1",
        "!(1 | 2)",
        "true && (1 | 2)",
        "false || (1 | 2)",
    ] {
        let source = format!("package demo\na: {expression}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected unresolved error for {expression}"
        );
    }
    // CUE short-circuits these operators, so the unresolved right operand is
    // never placed in a boolean context.
    let short_circuit = r#"
package demo
a: false && (1 | 2)
b: true || (1 | 2)
"#;
    let file = ostinato_lang::parse(short_circuit).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], false);
    assert_eq!(json["b"], true);
    let ok = r#"
package demo
a: (*1 | 2) == 1
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["a"], true);
}

#[test]
fn defaulted_disjunctions_work_in_select_index_and_for() {
    let source = r#"
package demo
a: (*{x: 1} | {x: 2}).x
b: (*[1, 2] | [3])[0]
c: [10, 20][*0 | 1]
x: *{a: 1} | {a: 2}
r: {
    for k, v in x {
        "\(k)": v
    }
}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["b"], 1);
    assert_eq!(json["c"], 10);
    assert_eq!(json["r"]["a"], 1);
}

#[test]
fn parenthesized_params_keep_overrides() {
    let source = r#"
package demo
params: ({name: string})
workflow: {name: "n-\(params.name)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("parenthesized params");
    assert_eq!(compiled.spec.name, "n-Alice");

    let definition = r#"
package demo
#P: {name: string}
params: (#P)
workflow: {name: "n-\(params.name)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        definition,
        &BTreeMap::from([("name".to_owned(), "Bob".to_owned())]),
    )
    .expect("parenthesized definition params");
    assert_eq!(compiled.spec.name, "n-Bob");
}

#[test]
fn parenthesized_definition_literal_keeps_optional_constraints() {
    let source = r#"
package demo
#P: ({name?: string})
x: #P & {name: 2}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let optional = r#"
package demo
#P: ({name?: string})
x: #P
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"], serde_json::json!({}));
}

#[test]
fn closed_definitions_reject_extra_fields_and_open_definitions_allow_them() {
    let closed = r#"
package demo
#D: {a: int}
x: #D & {a: 1, b: 2}
"#;
    let file = ostinato_lang::parse(closed).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let open = r#"
package demo
#D: {a: int, ...}
x: #D & {a: 1, b: 2}
"#;
    let file = ostinato_lang::parse(open).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["b"], 2);
}

#[test]
fn patterns_allow_and_constrain_extra_fields() {
    let any = r#"
package demo
#D: {[string]: int}
x: #D & {a: 1, b: 2}
"#;
    let file = ostinato_lang::parse(any).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["b"], 2);

    let bad = r#"
package demo
#D: {[string]: int}
x: #D & {a: "nope"}
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());

    let regex = r#"
package demo
#D: {[=~"^x"]: int}
x: #D & {x1: 1, x2: 2}
"#;
    let file = ostinato_lang::parse(regex).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["x2"], 2);

    let regex_bad = r#"
package demo
#D: {[=~"^x"]: int}
x: #D & {y: 1}
"#;
    let file = ostinato_lang::parse(regex_bad).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn patterns_apply_to_declared_fields() {
    let source = r#"
package demo
#D: {[string]: int, a: "nope"}
x: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn nested_definitions_are_closed_unless_opened() {
    let closed = r#"
package demo
#D: {inner: {a: int}}
x: #D & {inner: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(closed).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let open = r#"
package demo
#D: {inner: {a: int, ...}}
x: #D & {inner: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(open).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["inner"]["b"], 2);
}

#[test]
fn nested_patterns_constrain_nested_fields() {
    let ok = r#"
package demo
#D: {inner: {[string]: int}}
x: #D & {inner: {a: 1}}
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["inner"]["a"], 1);

    let bad = r#"
package demo
#D: {inner: {[string]: int}}
x: #D & {inner: {a: "nope"}}
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn optional_fields_are_pattern_constrained_when_materialized() {
    let ok = r#"
package demo
#D: {a?: int, [string]: int}
x: #D & {a: 1}
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["a"], 1);

    let bad = r#"
package demo
#D: {a?: int, [string]: int}
x: #D & {a: "nope"}
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn pattern_values_can_be_disjunctions() {
    let source = r#"
package demo
#D: {[string]: int | *1}
x: #D & {a: 2}
y: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["a"], 2);
    assert_eq!(json["y"], serde_json::json!({}));
}

#[test]
fn nested_struct_unification_is_closed_after_composition() {
    let ok = r#"
package demo
#D: {inner: {a: int} & {b: int}}
x: #D & {inner: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["inner"]["b"], 2);

    let referenced = r#"
package demo
base: {a: 1}
#D: {inner: base}
x: #D & {inner: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(referenced).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let referenced_open = r#"
package demo
base: {a: 1, ...}
#D: {inner: base}
x: #D & {inner: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(referenced_open).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["inner"]["b"], 2);
}

#[test]
fn closed_definition_intersections_check_every_operand() {
    let optional = r#"
package demo
#D: {a?: int}
#E: {}
x: #D & #E & {a: 1}
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let pattern = r#"
package demo
#D: {[=~"^x"]: int}
#E: {}
x: #D & #E & {x1: 1}
"#;
    let file = ostinato_lang::parse(pattern).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let overlap = r#"
package demo
#D: {[string]: int}
#E: {a: int}
x: #D & #E & {a: 1, b: 2}
"#;
    let file = ostinato_lang::parse(overlap).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let disjoint = r#"
package demo
#D: {[=~"^x"]: int}
#E: {[=~"^y"]: int}
x: #D & #E & {x1: 1}
"#;
    let file = ostinato_lang::parse(disjoint).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let valid = r#"
package demo
#D: {[string]: int}
#E: {a: int}
x: #D & #E & {a: 1}
"#;
    let file = ostinato_lang::parse(valid).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_ok());
}

#[test]
fn pattern_value_structs_are_closed_for_external_fields() {
    for source in [
        "#D: {[string]: {a: int}}\nx: #D & {k: {a: 1, b: 2}}",
        "#D: {[string]: {a: int}, ...}\nx: #D & {k: {a: 1, b: 2}}",
        "#D: {[string]: [{a: int}]}\nx: #D & {k: [{a: 1, b: 2}]}",
        "#D: {[string]: {a: int} & {b: int}}\nx: #D & {k: {a: 1, b: 2, c: 3}}",
    ] {
        let source = format!("package demo\n{source}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }

    let same_definition = r#"
package demo
#D: {[string]: {a: int}, k: {a: 1, b: 2}}
x: #D
"#;
    let file = ostinato_lang::parse(same_definition).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["k"]["b"], 2);
}

#[test]
fn non_struct_definition_values_close_nested_structs() {
    for source in [
        "#D: [{a: int}]\nx: #D & [{a: 1, b: 2}]",
        "#D: [...{a: int}]\nx: #D & [{a: 1, b: 2}]",
        "#D: [{a: int}, ...{a: int}]\nx: #D & [{a: 1, b: 2}]",
        "#D: {a: int} | {b: int}\nx: #D & {a: 1, c: 2}",
        "#D: *{a: int} | {b: int}\nx: #D & {a: 1, c: 2}",
    ] {
        let source = format!("package demo\n{source}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }
}

#[test]
fn optional_values_get_patterns_before_closing() {
    let source = r#"
package demo
#D: {[string]: {a: int}, k?: {b: 2}}
x: #D & {k: {b: 2}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json();
    assert!(json.is_none(), "a is an incomplete int in x.k");
}

#[test]
fn optional_pattern_conflicts_are_deferred_until_materialized() {
    let unmaterialized = r#"
package demo
#D: {[string]: string, k?: 1}
x: #D
"#;
    let file = ostinato_lang::parse(unmaterialized).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"], serde_json::json!({}));

    let materialized = r#"
package demo
#D: {[string]: string, k?: 1}
x: #D & {k: 1}
"#;
    let file = ostinato_lang::parse(materialized).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn multiple_matching_patterns_merge_before_closing() {
    let source = r#"
package demo
#D: {[string]: {a: int}, [=~"^k"]: {b: int}}
x: #D & {k: {a: 1, b: 2}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["k"]["b"], 2);
}

#[test]
fn derived_definitions_keep_referenced_closedness() {
    let source = r#"
package demo
#Base: {name: string}
#Derived: #Base & {extra: int}
d: #Derived & {name: "n", extra: 1}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn promoted_optional_fields_are_closed_by_other_patterns() {
    let source = r#"
package demo
#Pattern: {[string]: {label: string}}
#Opt: {k?: {note: string, ...}}
v: #Pattern & #Opt & {k: {label: "l", note: "n"}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let reverse = r#"
package demo
#Opt: {k?: {note: string, ...}}
#Pattern: {[string]: {label: string}}
v: #Opt & #Pattern & {k: {label: "l", note: "n"}}
"#;
    let file = ostinato_lang::parse(reverse).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn definition_disjunctions_keep_alternatives() {
    let source = r#"
package demo
#A: {a: int}
#B: {b: int}
#D: #A | #B
x: #D & {a: 1}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["a"], 1);

    let default = r#"
package demo
#A: {a: int}
#D: #A | *{b: 1}
x: #D
"#;
    let file = ostinato_lang::parse(default).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["b"], 1);

    let intersection = r#"
package demo
#A: {a: int}
#B: {a: int, b: int}
#D: (#A | #B) & {a: 1}
x: #D
"#;
    let file = ostinato_lang::parse(intersection).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(
        value.to_json().is_none(),
        "intersection is incomplete in CUE"
    );
}

#[test]
fn invalid_regex_on_optional_fields_is_eager() {
    let source = r#"
package demo
#D: {[=~"("]: int, k?: 1}
x: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("invalid regex");
    assert!(error.to_string().contains("invalid pattern regex"));

    let promoted = r#"
package demo
#A: {[=~"("]: int}
#B: {k?: 1}
x: #A & #B
"#;
    let file = ostinato_lang::parse(promoted).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn optional_value_conflicts_are_still_deferred() {
    let source = r#"
package demo
#D: {[string]: string, k?: 1}
x: #D
#L: {[string]: [int, int], j?: [1, 2, 3]}
y: #L
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"], serde_json::json!({}));
    assert_eq!(json["y"], serde_json::json!({}));
}

#[test]
fn invalid_regex_fails_even_when_optional_value_conflicts() {
    for source in [
        "#D: {[=~\"(\"]: int, k?: 1 & 2}",
        "#D: {[=~\"(\"]: int, k?: 1, k?: 2}",
    ] {
        let source = format!("package demo\n{source}\nx: #D\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("invalid regex");
        assert!(error.to_string().contains("invalid pattern regex"));
    }
}

#[test]
fn disjunction_dedupe_preserves_definition_metadata() {
    let source = r#"
package demo
#A: {a: int}
#B: {a: int, ...}
#D: #A | #B
x: #D & {a: 1, extra: 2}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["extra"], 2);

    let reverse = r#"
package demo
#A: {a: int}
#B: {a: int, ...}
#D: #B | #A
x: #D & {a: 1, extra: 2}
"#;
    let file = ostinato_lang::parse(reverse).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["extra"], 2);

    let optional = r#"
package demo
#A: {a: 1}
#B: {a: 1, o?: 1}
#D: #A | #B
x: #D
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(value.to_json().is_none());
}

#[test]
fn multi_expression_definitions_close_after_composition() {
    let source = r#"
package demo
#A: {a: int}
#B: {b: int}
#D: #A | #B
#D: {c?: int}
x: #D & {a: 1}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"]["a"], 1);
}

#[test]
fn composed_definitions_keep_referenced_closedness() {
    for source in [
        "#A: {a: 1}\n#B: {a: 1}\n#D: {o?: 1}\n#D: #A | #B\nx: #D & {o: 1}",
        "#A: {a: 1}\n#D: {[string]: int}\n#D: #A | #A\nx: #D & {b: 2}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect_err("closed definition must reject the field");
    }
}

#[test]
fn aliases_bind_field_values() {
    let source = r#"
package demo
X=foo: 1
bar: X
baz: X + 1
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["bar"], 1);
    assert_eq!(json["baz"], 2);
}

#[test]
fn aliases_work_in_definitions_and_nested_structs() {
    let source = r#"
package demo
#D: {X=a: 1, b: X}
x: #D
nested: {Y=b: 2, c: Y}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["b"], 1);
    assert_eq!(json["nested"]["c"], 2);
}

#[test]
fn aliases_can_bind_definitions_and_dynamic_labels() {
    let source = r#"
package demo
X=#D: {a: 1}
def: X
Y="dyn": 3
dynValue: Y
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["def"]["a"], 1);
    assert_eq!(json["dyn"], 3);
    assert_eq!(json["dynValue"], 3);
}

#[test]
fn invalid_aliases_are_rejected() {
    let unreferenced = r#"
package demo
X=a: 1
"#;
    let file = ostinato_lang::parse(unreferenced).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("unreferenced alias");
    assert!(error.to_string().contains("unreferenced alias"));

    let optional = r#"
package demo
X=a?: 1
b: X
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("optional alias");
    assert!(
        error
            .to_string()
            .contains("cannot reference optional field")
    );

    let conflict = r#"
package demo
X=a: 1
X: 2
b: X
"#;
    let file = ostinato_lang::parse(conflict).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("alias conflict");
    assert!(error.to_string().contains("both alias and field"));
}

#[test]
fn aliases_used_only_in_optional_definition_or_pattern_are_referenced() {
    let optional = r#"
package demo
X=a: 1
b?: X
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("optional use");
    assert_eq!(value.to_json().expect("json")["a"], 1);

    let definition = r#"
package demo
X=a: 1
#D: {v: X}
"#;
    let file = ostinato_lang::parse(definition).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("definition use");
    assert_eq!(value.to_json().expect("json")["a"], 1);

    let pattern = r#"
package demo
X=a: 1
[string]: X
"#;
    let file = ostinato_lang::parse(pattern).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("pattern use");
    assert_eq!(value.to_json().expect("json")["a"], 1);
}

#[test]
fn alias_scope_conflicts_and_duplicates_are_rejected() {
    let duplicate = r#"
package demo
X=a: 2
X=b: 3
c: X
"#;
    let file = ostinato_lang::parse(duplicate).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("duplicate alias");
    assert!(error.to_string().contains("redeclared"));

    for source in [
        "X=a: 1\nb: {X: 2}\nc: X",
        "b: {X=a: 1, c: X}\nX: 2",
        "X=a: 1\nif true {X: 3}\nb: X",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("alias/field conflict");
        assert!(error.to_string().contains("both alias and field"));
    }

    let siblings = r#"
package demo
b: {X=a: 1, c: X}
d: {X: 2}
"#;
    let file = ostinato_lang::parse(siblings).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_ok());
}

#[test]
fn unreferenced_alias_inside_definition_is_rejected() {
    let source = r#"
package demo
#D: {X=a: 1}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("unreferenced alias");
    assert!(error.to_string().contains("unreferenced alias"));
}

#[test]
fn inner_alias_shadows_an_enclosing_let() {
    let source = r#"
package demo
let X = 2
b: {X=a: 1, c: X}
d: X
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("inner alias");
    let json = value.to_json().expect("json");
    assert_eq!(json["b"]["c"], 1);
    assert_eq!(json["d"], 2);

    let conditional = r#"
package demo
let X = 2
if true {X=a: 1, c: X}
d: X
"#;
    let file = ostinato_lang::parse(conditional).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("conditional alias");
    assert_eq!(value.to_json().expect("json")["c"], 1);
}

#[test]
fn aliases_conflict_with_comprehension_bindings() {
    for source in [
        "X=a: 1\nfor X in [1] {b: X}\nc: X",
        "X=a: 1\nl: [for X in [1, 2] {X + 1}]",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("binding conflict");
        assert!(error.to_string().contains("both alias and field"));
    }

    let siblings = r#"
package demo
b: {for X in [1] {c: X}}
d: {X=e: 1, f: X}
"#;
    let file = ostinato_lang::parse(siblings).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_ok());
}

#[test]
fn aliases_and_lets_conflict_with_import_names() {
    let alias = r#"
package demo
strings=a: 1
c: strings
"#;
    let file = ostinato_lang::parse(alias).expect("parse");
    let imports = BTreeMap::from([(
        "strings".to_owned(),
        ostinato_lang::builtin_module("strings").expect("strings"),
    )]);
    let error = ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new())
        .expect_err("import alias conflict");
    assert!(error.to_string().contains("both alias and field"));

    let let_binding = r#"
package demo
let strings = 1
c: strings
"#;
    let file = ostinato_lang::parse(let_binding).expect("parse");
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn underscore_alias_is_rejected() {
    let source = r#"
package demo
_=a: 1
c: _
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("underscore alias");
    assert!(error.to_string().contains("cannot use _ as alias"));
}

#[test]
fn let_conflicts_and_unreferenced_lets_are_rejected() {
    for source in [
        "let X = 1\nlet X = 2\na: X",
        "let X = 1\na: 2",
        "let X = 1\nX: 2\nb: X",
        "let X = 1\nb: {X: 2}\nc: X",
        "let X = 1\nl: [for X in [1, 2] {X + 1}]",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }

    let used = r#"
package demo
let X = 1
b: {Y: 2}
c: X
"#;
    let file = ostinato_lang::parse(used).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("used let");
    assert_eq!(value.to_json().expect("json")["c"], 1);
}

#[test]
fn quoted_labels_do_not_conflict_with_aliases() {
    let source = r#"
package demo
X=a: 1
"X": 2
b: X
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("quoted label");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["X"], 2);
    assert_eq!(json["b"], 1);
}

#[test]
fn list_comprehensions_support_struct_bodies() {
    let source = r#"
package demo
l: [for i in [1, 2] {a: i, b: i}]
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(
        value.to_json().expect("json")["l"],
        serde_json::json!([{"a": 1, "b": 1}, {"a": 2, "b": 2}])
    );
}

#[test]
fn list_comprehension_bodies_can_start_with_brackets() {
    let source = r#"
package demo
pairs: [for i in [1, 2] {[i, i]}]
empty: [for i in [1] {[]}]
nested: [for i in [1, 2] {[for j in [3] {j + i}]}]
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["pairs"], serde_json::json!([[1, 1], [2, 2]]));
    assert_eq!(json["empty"], serde_json::json!([[]]));
    assert_eq!(json["nested"], serde_json::json!([[4], [5]]));
}

#[test]
fn list_comprehension_struct_bodies_still_support_patterns() {
    let source = r#"
package demo
all: [for name in ["a", "b"] {[string]: name}]
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(
        value.to_json().expect("json")["all"],
        serde_json::json!([{}, {}])
    );
}

#[test]
fn inner_lets_can_shadow_outer_lets_and_aliases() {
    let outer_let = r#"
package demo
let X = 2
b: {let X = X + 1, c: X}
d: X
"#;
    let file = ostinato_lang::parse(outer_let).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("inner let");
    let json = value.to_json().expect("json");
    assert_eq!(json["b"]["c"], 3);
    assert_eq!(json["d"], 2);

    let outer_alias = r#"
package demo
X=a: 2
b: {let X = X + 1, c: X}
d: X
"#;
    let file = ostinato_lang::parse(outer_alias).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("inner let over alias");
    let json = value.to_json().expect("json");
    assert_eq!(json["b"]["c"], 3);
    assert_eq!(json["d"], 2);
}

#[test]
fn let_cycles_are_rejected() {
    for source in [
        "let X = 1\na: {let Y = X, let X = Y, c: X}\nd: X",
        "let X = X\na: X",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("let cycle");
        assert!(
            error.to_string().contains("cyclic")
                || error.to_string().contains("undefined reference"),
            "{error}"
        );
    }
}

#[test]
fn let_shadowing_does_not_require_an_outer_reference() {
    let valid = r#"
package demo
let X = 1
b: {let X = X + 1, c: X}
"#;
    let file = ostinato_lang::parse(valid).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("outer let is used by the RHS");
    assert_eq!(value.to_json().expect("json")["b"]["c"], 2);

    let unused = r#"
package demo
let X = 1
b: {let X = X + 1}
d: X
"#;
    let file = ostinato_lang::parse(unused).expect("parse");
    let error = ostinato_lang::eval_file(&file).expect_err("inner let is unreferenced");
    assert!(error.to_string().contains("unreferenced"));
}

#[test]
fn undemanded_let_cycles_are_rejected_statically() {
    for source in [
        "workflow: {let Y = X, let X = Y, name: \"n\", steps: {s: {kind: \"noop\"}}}",
        "a: {let Y = X, let X = Y}\nd: 1",
        "a: {let Y = X, let X = Y, c?: X}\nd: 1",
        "#D: {let Y = X, let X = Y}\nd: 1",
        "let X = 1\na: {let Y = X, let X = Y}\nd: X",
        "let A = {if B {}}\nlet B = A\nd: 1",
        "let A = {for i in B {}}\nlet B = A\nd: 1",
        "let A = {x: B}\nlet B = A\nd: 1",
        "let A = [{x: B}]\nlet B = A\nd: 1",
        "let A = ({x: B})\nlet B = A\nd: 1",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("static let cycle");
        assert!(
            error.to_string().contains("cyclic"),
            "unexpected error for {source}: {error}"
        );
    }
}

#[test]
fn acyclic_let_chains_are_accepted() {
    let source = r#"
package demo
let A = 1
b: {
    let C = B + 1
    let B = A + 1
    c: C
}
d: A
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("acyclic chain");
    assert_eq!(value.to_json().expect("json")["b"]["c"], 3);
}

#[test]
fn quoted_labels_are_not_identifier_bindings() {
    let literal = r#"
package demo
let X = 2
a: {"X": 1, "Z": X}
b: {"X": X}
"#;
    let file = ostinato_lang::parse(literal).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"]["X"], 1);
    assert_eq!(json["a"]["Z"], 2);
    assert_eq!(json["b"]["X"], 2);

    let alias = r#"
package demo
X=a: 1
b: {"X": 2, Y: X}
"#;
    let file = ostinato_lang::parse(alias).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["b"]["X"], 2);
    assert_eq!(json["b"]["Y"], 1);

    let dynamic = r#"
package demo
let X = 2
a: {("X"): 1, Z: X}
"#;
    let file = ostinato_lang::parse(dynamic).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"]["X"], 1);
    assert_eq!(json["a"]["Z"], 2);
}

#[test]
fn quoted_hash_labels_are_distinct_from_definitions() {
    let source = r##"
package demo
params: {"#D": 1, #D: 2}
workflow: {name: "n", steps: {s: {kind: "noop"}}}
"##;
    let compiled = ostinato_lang::compile(source).expect("quoted label and definition");
    let params = compiled.resolve_params(&BTreeMap::new()).expect("params");
    assert_eq!(params["#D"], 1);

    let selector = r##"
package demo
#P: {"#h": 9, a: 2}
x: #P.#h
"##;
    let file = ostinato_lang::parse(selector).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let quoted_export = r##"
package demo
#P: {"#h": 9, a: 2}
x: #P
"##;
    let file = ostinato_lang::parse(quoted_export).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["#h"], 9);
    assert_eq!(json["x"]["a"], 2);
}

#[test]
fn quoted_parameter_names_use_identifier_values() {
    let source = r#"
package demo
let X = "real"
params: {"X": "fake", "Z": X}
workflow: {name: "\(params.Z)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile(source).expect("quoted params");
    assert_eq!(compiled.spec.name, "real");
}

#[test]
fn aliases_to_quoted_hash_labels_resolve_the_regular_field() {
    let source = r##"
package demo
A="#D": 9
#D: 8
params: {v: A, w: #D}
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["v"], 9);
    assert_eq!(json["params"]["w"], 8);

    let dynamic = r##"
package demo
A=("#D"): 9
#D: 8
v: A
"##;
    let file = ostinato_lang::parse(dynamic).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["v"], 9);

    let nested = r##"
package demo
#P: {A="#h": 5, #h: 6, z: A}
w: #P
"##;
    let file = ostinato_lang::parse(nested).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["w"]["#h"], 5);
    assert_eq!(json["w"]["z"], 5);
}

#[test]
fn string_index_addresses_regular_fields_only() {
    let source = r##"
package demo
#P: {#D: 8, "#D": 9}
a: #P["#D"]
b: #P.#D
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 9);
    assert_eq!(json["b"], 8);

    let missing = r##"
package demo
#P: {#D: 8}
x: #P["#D"]
"##;
    let file = ostinato_lang::parse(missing).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn identifier_select_does_not_reach_quoted_hash_fields() {
    let source = r##"
package demo
a: {"#D": 9}
x: a.#D
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let normal = r##"
package demo
a: {"X": 9}
x: a.X
"##;
    let file = ostinato_lang::parse(normal).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["x"], 9);
}

#[test]
fn imported_packages_keep_quoted_hash_fields_next_to_definitions() {
    let lib = r##"
package lib
A="#D": 9
#D: 8
use: A
"##;
    let lib_file = ostinato_lang::parse(lib).expect("lib parse");
    let lib_value = ostinato_lang::eval_file_with_imports_mode(
        &lib_file,
        &BTreeMap::new(),
        &BTreeMap::new(),
        true,
    )
    .expect("lib eval");

    let main = r##"
package main
params: {q: lib["#D"], d: lib.#D, u: lib.use}
"##;
    let main_file = ostinato_lang::parse(main).expect("main parse");
    let imports = BTreeMap::from([("lib".to_owned(), lib_value)]);
    let value = ostinato_lang::eval_file_with_imports(&main_file, &imports, &BTreeMap::new())
        .expect("main");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["q"], 9);
    assert_eq!(json["params"]["d"], 8);
    assert_eq!(json["params"]["u"], 9);
}

#[test]
fn comprehension_bodies_keep_quoted_hash_fields_next_to_definitions() {
    let source = r##"
package demo
#P: {
    #D: 8
    for _, x in [1] {"#D": x}
}
params: {v: #P["#D"], w: #P.#D}

#Q: {
    #E: 7
    if true {"#E": 9}
}
other: {v: #Q["#E"], w: #Q.#E}
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["v"], 1);
    assert_eq!(json["params"]["w"], 8);
    assert_eq!(json["other"]["v"], 9);
    assert_eq!(json["other"]["w"], 7);
}

#[test]
fn definitions_do_not_admit_regular_fields_of_the_same_label() {
    for source in [
        "#P: {#D: 8}\na: #P & {\"#D\": 9}",
        "#P: {Q: {#D: 8}}\na: #P & {Q: {\"#D\": 9}}",
        "#P: {#D: 8}\n#P: {x: 1}\na: #P & {\"#D\": 9}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "regular field must not be admitted: {source}"
        );
    }

    let generated = r##"
package demo
#P: {#D: 8, if true {"#D"?: 9}}
params: {w: #P.#D}
"##;
    let file = ostinato_lang::parse(generated).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated optional");
    assert_eq!(value.to_json().expect("json")["params"]["w"], 8);

    let optional_reference = r##"
package demo
#P: {#D: 8, if true {"#D"?: 9}}
v: #P["#D"]
"##;
    let file = ostinato_lang::parse(optional_reference).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn let_cycles_through_required_definitions_are_rejected() {
    for source in [
        "let X = #A\n#A: {v: X}\nparams: {v: 1}",
        "let X = A\nA=#D: {v: X}\nparams: {v: 1}",
        "A=#D: {v: B}\nB=#E: {v: A}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("definition cycle");
        assert!(error.to_string().contains("cyclic"), "{error}");
    }

    let optional = r#"
package demo
let X = f
f?: X
params: {v: 1}
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_ok());
}

#[test]
fn generated_quoted_optionals_target_the_regular_field() {
    let source = r##"
package demo
#P: {#D: 8, for _, x in [1] {"#D": x}, for _, x in [1] {"#D"?: x}}
params: {v: #P["#D"], w: #P.#D}
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated quoted optional");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["v"], 1);
    assert_eq!(json["params"]["w"], 8);

    let conflict = r##"
package demo
#P: {#D: 8, "#D": 1, "#D"?: 8}
params: {v: #P["#D"], w: #P.#D}
"##;
    let file = ostinato_lang::parse(conflict).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn dead_comprehension_bodies_do_not_create_required_cycles() {
    for source in [
        "let X = #A\n#A: {if false {v: X}}\nparams: {v: 1}",
        "let C = false\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {if 1 > 2 {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {for k in [] {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {for k, v in {} {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {v: [for k in [] {X}]}\nparams: {v: 1}",
        "let X = #A\n#A: {if false {#B: {v: X}}}\nparams: {v: 1}",
        "let X = #A\n#A: {for k in [1] if false {v: X}}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_ok(),
            "dead body must not create a cycle: {source}"
        );
    }

    for source in [
        "let X = #A\n#A: {if true {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {for k in [1] {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {v: [for k in [1] {X}]}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "live body must keep the cycle: {source}"
        );
    }
}

#[test]
fn generated_quoted_optionals_do_not_touch_materialized_definitions() {
    let source = r##"
package demo
#P: {
    #D: 8
    if true {a: #D}
    if true {"#D"?: int}
}
params: {a: #P.a, w: #P.#D}
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated optional after definition");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["a"], 8);
    assert_eq!(json["params"]["w"], 8);

    let ordered = r##"
package demo
#P: {#D: 8, if #D == 8 {"#D"?: int}, if true {"#D": 9}}
params: {v: #P["#D"], w: #P.#D}
"##;
    let file = ostinato_lang::parse(ordered).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("order independent");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["v"], 9);
    assert_eq!(json["params"]["w"], 8);
}

#[test]
fn live_struct_sources_and_constant_conditions_keep_cycles() {
    for source in [
        "let X = #A\n#A: {for k, x in {a: 1} {w: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {if 1 == 1 {v: X}}\nparams: {v: 1}",
        "let C = true\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let C = 1 == 1\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let C = !false\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let C = true && true\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let S = {a: 1}\nlet X = #A\n#A: {for k, x in S {w: X}}\nparams: {v: 1}",
        "let C = 1 == 1\nlet X = #A\n#A: {v: [for i in [1] if C {X}]}\nparams: {v: 1}",
        "let X = #A\n#A: {if \"a\" < \"b\" {v: X}}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "live body must keep the cycle: {source}"
        );
    }

    for source in [
        "let X = #A\n#A: {for k, x in {} {w: X}}\nparams: {v: 1}",
        "let S = {}\nlet X = #A\n#A: {for k, x in S {w: X}}\nparams: {v: 1}",
        "let C = 1 == 2\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let C = 1 == 1\nlet X = #A\n#A: {if C && false {v: X}}\nparams: {v: 1}",
        "let X = #A\n#Helper: {ok: bool, if ok && true {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {if 1 == 2 {v: X}}\nparams: {v: 1}",
        "let C = false\nlet X = #A\n#A: {if C {v: X}}\nparams: {v: 1}",
        "let X = #A\n#A: {if \"b\" < \"a\" {v: X}}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_ok(),
            "dead body must not create a cycle: {source}"
        );
    }
}

#[test]
fn alias_shadowing_keeps_dead_bodies_dead() {
    for source in [
        "let C = 1 == 1\nc: C\nlet X = #A\n#A: {C = x: false, if C {v: X}}\nparams: {v: 1}",
        "let C = !false\nc: C\nlet X = #A\n#A: {C = x: false, if C {v: X}}\nparams: {v: 1}",
        "let C = true && true\nc: C\nlet X = #A\n#A: {C = x: false, if C {v: X}}\nparams: {v: 1}",
        "let S = {a: 1}\ns: S\nlet X = #A\n#A: {S = x: {}, v: [for k in S {X}]}\nparams: {v: 1}",
        "let S = {a: 1}\ns: S\nlet X = #A\n#A: {S = x: {}, for k in S {v: X}}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_ok(),
            "alias shadowing must keep the body dead: {source}"
        );
    }
}

#[test]
fn unification_sources_are_live_when_either_side_is_live() {
    for source in [
        "let S = {a: 1} & {}\nlet X = #A\n#A: {for k, v in S {v: X}}\nparams: {v: 1}",
        "let S = {} & {a: 1}\nlet X = #A\n#A: {for k, v in S {v: X}}\nparams: {v: 1}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "live unification must keep the cycle: {source}"
        );
    }
}

#[test]
fn struct_literal_shadowing_keeps_dead_bodies_dead() {
    let dead = [
        "let C = true\nc: C\nlet S = {let C = false, if C {a: 1}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {C = #x: false, if C {a: 1}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
    ];
    for source in dead {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let value = ostinato_lang::eval_file(&file).expect("dead body must stay dead");
        assert_eq!(value.to_json().expect("json")["s"], serde_json::json!({}));
    }

    let live = "let C = false\nc: C\nlet S = {let C = true, if C {a: 1}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A";
    let file = ostinato_lang::parse(&format!("package demo\n{live}\n")).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "inner live binding must keep the cycle"
    );
}

#[test]
fn nested_comprehension_bodies_shadow_in_static_liveness() {
    for source in [
        "let C = true\nc: C\nlet S = {for x in [1] {let C = false, if C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {if true {let C = false, if C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {for x in [1] {C = #y: false, if C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {for x in [1] {for y in [1] {let C = false, if C {a: 1}}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {for x in [1] {let C = {}, for k in C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {for x in [1] if true {let C = false, if C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
        "let C = true\nc: C\nlet S = {for x in [1] {let C = false, if C == true {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let value = ostinato_lang::eval_file(&file).expect("dead nested body must stay dead");
        assert_eq!(value.to_json().expect("json")["s"], serde_json::json!({}));
    }

    let live = "let C = false\nc: C\nlet S = {for x in [1] {let C = true, if C {a: 1}}}\ns: S\nlet X = #A\n#A: {for k in S {v: X}}\nout: #A";
    let file = ostinato_lang::parse(&format!("package demo\n{live}\n")).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "live nested binding must keep the cycle"
    );
}

#[test]
fn bounds_narrow_concrete_values_and_types() {
    let source = r#"
package demo
a: >=5 & 7
b: >=1 & <=10 & !=5 & 6
c: =~"^ab" & "abc"
d: !~"^ab" & "xyz"
e: >=1.5 & <=2.5 & 2
f: >1.0 & <2.0 & 1.5
g: >=5 & (1 | 7)
h: >"a" & "b"
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 7);
    assert_eq!(json["b"], 6);
    assert_eq!(json["c"], "abc");
    assert_eq!(json["d"], "xyz");
    assert_eq!(json["e"], 2);
    assert_eq!(json["f"], 1.5);
    assert_eq!(json["g"], 7);
    assert_eq!(json["h"], "b");
    assert!(
        json.get("i").is_none(),
        "bound over a type stays incomplete"
    );
}

#[test]
fn bounds_reject_out_of_range_values() {
    for source in [
        "a: >=5 & 3",
        "a: >=1 & <=10 & !=5 & 5",
        "a: =~\"^ab\" & \"xyz\"",
        "a: !~\"^ab\" & \"abc\"",
        "a: >1.0 & <2.0 & 2.0",
        "a: >=int & 1",
        "a: =~\"^ab\" & 3",
        "a: >=5 & \"x\"",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected bound conflict for {source}"
        );
    }
}

#[test]
fn bound_disjunctions_and_defaults_are_lattice_aware() {
    let source = r#"
package demo
a: *1 | >=5
b: >=5 | 10
c: (*>=5 | 10) & 7
d: >=5 & (>=6 | 7)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json();
    assert!(
        json.is_none(),
        "b is incomplete, so the export is incomplete"
    );

    let defaulted = r#"
package demo
a: *1 | >=5
c: (*>=5 | 10) & 7
"#;
    let file = ostinato_lang::parse(defaulted).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["c"], 7);

    let partial = r#"
package demo
d: >=5 & (>=6 | 7)
"#;
    let file = ostinato_lang::parse(partial).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(
        value.to_json().is_none(),
        "bound alternative stays incomplete"
    );
}

#[test]
fn decimal_arithmetic_is_exact() {
    let source = r#"
package demo
a: 1.1 + 2.2
b: 0.1 + 0.2 == 0.3
c: 1.0 == 1
d: 1 / 2
e: 123456789012345678.1 + 0.2 == 123456789012345678.3
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"].to_string(), "3.3");
    assert_eq!(json["b"], true);
    assert_eq!(json["c"], true);
    assert_eq!(json["d"].to_string(), "0.5");
    assert_eq!(json["e"], true);
}

#[test]
fn bounds_validate_operands_types_and_intervals() {
    let ok = r#"
package demo
a: >="a" & string & "b"
b: >=5 & <=5 & 5
c: >=5 & int & 7
d: !=null & "x"
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "b");
    assert_eq!(json["b"], 5);
    assert_eq!(json["c"], 7);
    assert_eq!(json["d"], "x");

    for source in [
        "a: >=5 & int & 7.5",
        "a: int & >=5 & 7.5",
        "a: >=5 & float & 5",
        "a: !=5 & \"a\"",
        "a: !=5 & true",
        "a: !=5 & {x: 1}",
        "a: !=\"a\" & 5",
        "a: !=5 & string",
        "a: >=5 & <5",
        "a: >5 & <=5",
        "a: >=5 & <3",
        "a: >=5 & >\"a\"",
        "a: >=true",
        "a: >=null",
        "a: =~true",
        "a: >=5 & >=true",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected bound error for {source}"
        );
    }

    for source in ["a: >=true | 3", "a: =~true | 3", "a: (>=5 & <3) | 7"] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let value = ostinato_lang::eval_file(&file).expect("disjunction skips bottom bound");
        assert_eq!(
            value.to_json().expect("json")["a"],
            3.max(value.to_json().expect("json")["a"].as_i64().unwrap_or(3))
                .min(7)
        );
    }
}

#[test]
fn division_by_zero_is_an_error_not_a_panic() {
    for source in ["x: 1.0 / 0", "x: 1 / 0.0", "x: 0.0 / 0", "x: 1e3 / 0"] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let error = ostinato_lang::eval_file(&file).expect_err("division by zero");
        assert!(error.to_string().contains("division by zero"), "{error}");
    }
}

#[test]
fn numeric_equality_and_float_export_match_cue() {
    let source = r#"
package demo
x: 1.0 == 1
a: 10 / 2
b: 1e3
c: 1.5e2
d: 1.0 * 0
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"], true);
    assert_eq!(json["a"].to_string(), "5.0");
    assert_eq!(json["b"].to_string(), "1e+3");
    assert_eq!(json["c"].to_string(), "1.5e+2");
    assert_eq!(json["d"].to_string(), "0.0");

    let bound = r#"
package demo
a: !=5 & 5.0
"#;
    let file = ostinato_lang::parse(bound).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn numeric_equality_is_scalar_only() {
    let scalar = r#"
package demo
a: 1.0 == 1
b: 1 == 1.0
"#;
    let file = ostinato_lang::parse(scalar).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], true);
    assert_eq!(json["b"], true);

    let composites = r#"
package demo
a: [1] == [1.0]
b: {x: 1} == {x: 1.0}
"#;
    let file = ostinato_lang::parse(composites).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], false);
    assert_eq!(json["b"], false);

    let disjoint = r#"
package demo
a: 1 | *1.0
b: 1.0 | *1
"#;
    let file = ostinato_lang::parse(disjoint).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"].to_string(), "1.0");
    assert_eq!(json["b"].to_string(), "1");

    let incomplete = r#"
package demo
a: 1 | 1.0
"#;
    let file = ostinato_lang::parse(incomplete).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert!(value.to_json().is_none());

    for source in ["a: \"\\(1 | 1.0)\"", "a: (1 | 1.0) + 1"] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }
}

#[test]
fn integer_bound_intervals_respect_discreteness() {
    for source in [
        "a: int & >5 & <6",
        "a: int & >=5.5 & <=5.5",
        "a: int & >5.0 & <6.0",
        "a: int & >=5.1 & <=5.9",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected empty integer interval for {source}"
        );
    }

    for (source, expected) in [
        ("a: int & >5 & <=6 & 6", 6),
        ("a: int & >=5.5 & <6.5 & 6", 6),
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        let value = ostinato_lang::eval_file(&file).expect("valid integer interval");
        assert_eq!(value.to_json().expect("json")["a"], expected);
    }

    let narrowed = r#"
package demo
a: (int & >5 & <6) | 7
"#;
    let file = ostinato_lang::parse(narrowed).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    assert_eq!(value.to_json().expect("json")["a"], 7);
}

#[test]
fn regexp_math_and_time_builtins_match_cue() {
    let source = r#"
package demo
import "regexp"
import "math"
import "time"

rm: regexp.Match("^a", "abc")
rf: regexp.Find("b+", "abbc")
rfa: regexp.FindAll("b+", "abbcbb", -1)
rr: regexp.ReplaceAll("b+", "abbc", "X")
rq: regexp.QuoteMeta("a.b")
rv: regexp.Valid("^a")

ma: math.Abs(-2.5)
mf: math.Floor(1.9)
mc: math.Ceil(1.1)
mr: math.Round(1.5)
mt: math.Trunc(-1.9)
ms: math.Sqrt(4)
mp: math.Pow(2, 10)
mpi: math.Pi

tu: time.Unix(0, 0)
tu2: time.Unix(1580000000, 0)
tp: time.Parse("2006-01-02", "2020-01-02")
tpd: time.ParseDuration("1h30m")
td: time.Duration("1h")
tf: time.Format(time.Unix(0, 0), "2006-01-02T15:04:05Z07:00")

bytes1: 'abc'
bytes2: '\x00\xff'
bytesLen: len(bytes1)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    for module in ["regexp", "math", "time"] {
        imports.insert(
            module.to_owned(),
            ostinato_lang::builtin_module(module).expect("builtin module"),
        );
    }
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["rm"], true);
    assert_eq!(json["rf"], "bb");
    assert_eq!(json["rfa"], serde_json::json!(["bb", "bb"]));
    assert_eq!(json["rr"], "aXc");
    assert_eq!(json["rq"], "a\\.b");
    assert_eq!(json["rv"], true);
    assert_eq!(json["ma"], 2.5);
    assert_eq!(json["mf"], 1);
    assert_eq!(json["mc"], 2);
    assert_eq!(json["mr"], 2);
    assert_eq!(json["mt"], -1);
    assert_eq!(json["ms"], 2);
    assert_eq!(json["mp"], 1024);
    assert!(
        json["mpi"]
            .as_f64()
            .is_some_and(|value| (value - std::f64::consts::PI).abs() < 1e-12)
    );
    assert_eq!(json["tu"], "1970-01-01T00:00:00Z");
    assert_eq!(json["tu2"], "2020-01-26T00:53:20Z");
    assert_eq!(json["tp"], "2020-01-02T00:00:00Z");
    assert_eq!(json["tpd"], 5_400_000_000_000i64);
    assert_eq!(json["td"], true);
    assert_eq!(json["tf"], true);
    assert_eq!(json["bytes1"], "YWJj");
    assert_eq!(json["bytes2"], "AP8=");
    assert_eq!(json["bytesLen"], 3);

    let bad = r#"
package demo
import "regexp"
a: regexp.Valid("[")
"#;
    let file = ostinato_lang::parse(bad).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("builtin module"),
    );
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn attributes_parse_in_every_cue_position() {
    let source = r#"
package demo
params: {
    x: 1 @if(true)
    @tag(x)
}
a: 2 @deprecated("old")
@tag(a)
b: 3
"#;
    let file = ostinato_lang::parse(source).expect("parse attributes");
    let value = ostinato_lang::eval_file(&file).expect("attributes are metadata");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["x"], 1);
    assert_eq!(json["b"], 3);
}

#[test]
fn bytes_literals_use_go_escape_rules() {
    let source = r#"
package demo
bell: '\a'
octal: '\101'
unicode: '\u00e9'
interp: "a\('bc')d"
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["bell"], "Bw==");
    assert_eq!(json["octal"], "QQ==");
    assert_eq!(json["unicode"], "w6k=");
    assert_eq!(json["interp"], "abcd");

    for literal in ["'\\q'", "'\\0'", "'\\\"'"] {
        let source = format!("package demo\na: {literal}\n");
        assert!(ostinato_lang::parse(&source).is_err(), "{literal}");
    }
}

#[test]
fn regexp_quote_find_and_re2_classes_match_cue() {
    let source = r#"
package demo
import "regexp"
q: regexp.QuoteMeta("a b-c.d+e#&~")
m: regexp.Match("\\Qa.b\\E", "a.b")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("regexp"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["q"], "a b-c\\.d\\+e#&~");
    assert_eq!(json["m"], true);

    for source in [
        "a: regexp.Find(\"z\", \"abc\")",
        "a: regexp.FindAll(\"z\", \"abc\", -1)",
        "a: regexp.FindAll(\"b+\", \"abbcbb\", 0)",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"regexp\"\n{source}\n"))
            .expect("parse");
        assert!(
            ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn math_rounding_is_decimal_exact() {
    let source = r#"
package demo
import "math"
f: math.Floor(1.99999999999999999)
c: math.Ceil(1.00000000000000001)
r: math.Round(0.49999999999999999)
t: math.Trunc(2.99999999999999999)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "math".to_owned(),
        ostinato_lang::builtin_module("math").expect("math"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["f"], 1);
    assert_eq!(json["c"], 2);
    assert_eq!(json["r"], 0);
    assert_eq!(json["t"], 2);
}

#[test]
fn time_helpers_handle_fractions_signs_and_nanos() {
    let source = r#"
package demo
import "time"
u1: time.Unix(0, -1)
u2: time.Unix(1, 500000000)
d1: time.ParseDuration("-1h")
d2: time.ParseDuration("1.5h")
d3: time.ParseDuration("0")
p1: time.Parse("15:04:05", "12:30:00")
p2: time.Parse("2006-01-02 15:04:05", "2020-01-02 12:30:00")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["u1"], "1969-12-31T23:59:59.999999999Z");
    assert_eq!(json["u2"], "1970-01-01T00:00:01.5Z");
    assert_eq!(json["d1"], -3_600_000_000_000i64);
    assert_eq!(json["d2"], 5_400_000_000_000i64);
    assert_eq!(json["d3"], 0);
    assert_eq!(json["p1"], "0000-01-01T12:30:00Z");
    assert_eq!(json["p2"], "2020-01-02T12:30:00Z");

    for source in [
        "a: time.Format(\"1970-01-01T00:00:00Z\", \"2006-01-02\")",
        "a: time.Duration(\"abc\")",
        "a: time.ParseDuration(\"abc\")",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(
            ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn string_escapes_are_decoded_and_invalid_ones_rejected() {
    let source = r#"
package demo
a: "caf\u00e9"
b: "a\\(b)"
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "café");
    assert_eq!(json["b"], "a\\(b)");

    for source in [r#"a: "x\qy""#, r#"a: "x\x41y""#, r#"a: "x\101y""#] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n"));
        assert!(file.is_err(), "{source}");
    }
}

#[test]
fn time_fractional_layouts_and_duration_edges() {
    let source = r#"
package demo
import "time"
a: time.Parse("15:04:05", "12:30:00.5")
b: time.Format("12:30:00.5", "15:04:05")
c: time.Parse("2006-01-02 15:04:05", "2020-01-02 12:30:00.25")
d: time.ParseDuration("9007199254740993ns")
e: time.ParseDuration("-0")
f: time.ParseDuration("1μs")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "0000-01-01T12:30:00.5Z");
    assert_eq!(json["b"], true);
    assert_eq!(json["c"], "2020-01-02T12:30:00.25Z");
    assert_eq!(json["d"], 9_007_199_254_740_993i64);
    assert_eq!(json["e"], 0);
    assert_eq!(json["f"], 1_000);

    for source in [
        "a: time.ParseDuration(\"2562048h\")",
        "a: time.ParseDuration(\" 1h\")",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn escaped_patterns_and_quoted_labels_decode_correctly() {
    let pattern = r#"
package demo
params: {d: {[=~"\\d+"]: 1, "123": 2}}
"#;
    let file = ostinato_lang::parse(pattern).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "pattern must match 123"
    );

    let labels = r#"
package demo
params: {p: {"a\\b": 1, "caf\u00e9": 2}}
"#;
    let file = ostinato_lang::parse(labels).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["p"]["a\\b"], 1);
    assert_eq!(json["params"]["p"]["café"], 2);
}

#[test]
fn unicode_surrogates_and_string_escape_edges() {
    let source = r#"
package demo
a: "x\uD83D\uDE00y"
b: len('x\uD83D\uDE00y')
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "x😀y");
    assert_eq!(json["b"], 6);

    let escaped_quote = r#"package demo
a: "x\'y"
"#;
    assert!(ostinato_lang::parse(escaped_quote).is_err());
}

#[test]
fn time_garbage_and_duration_fraction_edges() {
    let source = r#"
package demo
import "time"
a: time.ParseDuration(".5s")
b: time.ParseDuration("1.s")
c: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02T12:30:00,25Z")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 500_000_000);
    assert_eq!(json["b"], 1_000_000_000);
    assert_eq!(json["c"], "2020-01-02T12:30:00.25Z");

    for source in [
        "a: time.Parse(\"15:04:05\", \"12:34:56.5xyz\")",
        "a: time.Format(\"12:34:56.5xyz\", \"15:04:05\")",
        "a: time.Parse(\"15:04:05\", \"12:34:56.\")",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn time_layouts_enforce_go_field_widths() {
    let source = r#"
package demo
import "time"
a: time.Parse("2006-01-02", "2020-01-02")
b: time.Parse("15:04:05", "12:30:05")
c: time.Parse("3:04PM", "3:04PM")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    assert_eq!(value.to_json().expect("json")["a"], "2020-01-02T00:00:00Z");

    for source in [
        "a: time.Parse(\"2006-01-02\", \"2020-1-2\")",
        "a: time.Parse(\"2006-01-02\", \"202-01-02\")",
        "a: time.Parse(\"15:04:05\", \"12:3:04\")",
        "a: time.Parse(\"15:04:05\", \"12:30:5\")",
        "a: time.Parse(\"3:04PM\", \"3:04pm\")",
        "a: time.Format(\"2020-1-2\", \"2006-01-02\")",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn open_list_prefix_values_export_their_prefix() {
    let source = r#"
package demo
params: {v: [1, 2, ...int]}
needs: ["a", ...string]
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["v"], serde_json::json!([1, 2]));
    assert_eq!(json["needs"], serde_json::json!(["a"]));
}

#[test]
fn time_rfc3339_and_leap_seconds_are_strict() {
    let source = r#"
package demo
import "time"
a: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02T12:30:00Z")
b: time.Parse("3:04PM", "0:04AM")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    assert_eq!(value.to_json().expect("json")["a"], "2020-01-02T12:30:00Z");

    for source in [
        r#"a: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02 12:30:00Z")"#,
        r#"a: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02t12:30:00Z")"#,
        r#"a: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02T12:30:00z")"#,
        r#"a: time.Parse("2006-01-02T15:04:05Z07:00", "2020-01-02T23:59:60Z")"#,
        r#"a: time.Parse("15:04:05", "23:59:60")"#,
        r#"a: time.Parse("3:04PM", "13:04PM")"#,
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn open_list_prefixes_work_as_lists() {
    let source = r#"
package demo
import "list"
same: [1, ...int] == [1]
different: [1, ...int] == [...int]
count: len([1, 2, ...int])
second: [10, 20, ...int][1]
looped: [for v in [1, 2, ...int] {v}]
concatenated: list.Concat([[1, ...int], [2]])
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "list".to_owned(),
        ostinato_lang::builtin_module("list").expect("list"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["same"], true);
    assert_eq!(json["different"], false);
    assert_eq!(json["count"], 2);
    assert_eq!(json["second"], 20);
    assert_eq!(json["looped"], serde_json::json!([1, 2]));
    assert_eq!(json["concatenated"], serde_json::json!([1, 2]));

    for source in [
        "a: \"v=\\([1, 2, ...int])\"",
        "a: \"v=\\([1, 2])\"",
        "a: \"v=\\({x: 1})\"",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }
}

#[test]
fn bare_open_lists_behave_as_empty_lists() {
    let source = r#"
package demo
import "list"
count: len([...int])
same: [...int] == []
cross: [...int] == [...string]
looped: [for v in [...int] {v}]
concatenated: list.Concat([[...int], [2]])
params: {tags: [...string]}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "list".to_owned(),
        ostinato_lang::builtin_module("list").expect("list"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["count"], 0);
    assert_eq!(json["same"], true);
    assert_eq!(json["cross"], true);
    assert_eq!(json["looped"], serde_json::json!([]));
    assert_eq!(json["concatenated"], serde_json::json!([2]));
    assert_eq!(json["params"]["tags"], serde_json::json!([]));
}

#[test]
fn regexp_uses_ascii_classes_and_repeat_limits() {
    let source = r#"
package demo
import "regexp"
word: regexp.Match("^\\w+$", "é")
digit: regexp.Match("^\\d+$", "١٢٣")
boundary: regexp.Match("\\bfoo\\b", "éfooé")
ascii: regexp.Match("^\\w+$", "abc_1")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("regexp"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["word"], false);
    assert_eq!(json["digit"], false);
    assert_eq!(json["boundary"], true);
    assert_eq!(json["ascii"], true);

    let too_large = r#"
package demo
import "regexp"
a: regexp.Valid("a{1001}")
"#;
    let file = ostinato_lang::parse(too_large).expect("parse");
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn escaped_slash_case_mapping_and_decimal_ranges() {
    let source = "package demo\nimport \"strings\"\nimport \"list\"\na: \"a\\/b\"\nb: 'a\\/b'\nt: strings.ToLower(\"İ\")\ng: strings.ToUpper(\"ᾀ\")\nr: list.Range(0, 3.5, 1)\ns: list.Range(1, 2, 0.5)\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    for module in ["strings", "list"] {
        imports.insert(
            module.to_owned(),
            ostinato_lang::builtin_module(module).expect("module"),
        );
    }
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "a/b");
    assert_eq!(json["b"], "YS9i");
    assert_eq!(json["t"], "i");
    assert_eq!(json["g"], "ᾈ");
    assert_eq!(json["r"], serde_json::json!([0, 1, 2, 3]));
    assert_eq!(json["s"], serde_json::json!([1, 1.5]));

    for source in [
        "a: list.Range(0, 3, -1)",
        "a: list.Range(3, 0, 1)",
        "a: list.Range(0, 3, 0)",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"list\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn regex_class_complements_and_repeat_scanner_are_re2_compatible() {
    let source = r#"
package demo
import "regexp"
a: regexp.Match("^[\\D]+$", "١٢")
b: regexp.Match("^[\\W]+$", "é")
c: regexp.Match("^[\\S]+$", " ")
d: regexp.Valid("[a{1001}]")
e: regexp.Valid("\\Q{1001}\\E")
f: regexp.Valid("\\{1001\\}")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("regexp"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], true);
    assert_eq!(json["b"], true);
    assert_eq!(json["c"], true);
    assert_eq!(json["d"], true);
    assert_eq!(json["e"], true);
    assert_eq!(json["f"], true);

    for source in [
        r#"a: regexp.Valid("[\\b]")"#,
        r#"a: regexp.Valid("[\\Qa-z\\E]")"#,
        r#"a: regexp.Valid("\\{\\d{1001}\\}")"#,
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"regexp\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn params_siblings_numeric_literals_and_bytes_concat() {
    let source = r#"
package demo
params: {host: string | *"example.com", url: "http://\(params.host)/api"}
a: .5
b: 5.
c: 1.e2
d: >=.5 & <1 & 0.75
e: 'a' + 'b'
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["url"], "http://example.com/api");
    assert_eq!(json["a"], 0.5);
    assert_eq!(json["b"], 5.0);
    assert_eq!(json["c"], 100.0);
    assert_eq!(json["d"], 0.75);
    assert_eq!(json["e"], "YWI=");

    let range = r#"
package demo
import "list"
a: list.Range(0, 2, 1.0)
b: list.Range(0, 2, 1.0) == [0, 1]
c: "\(list.Range(0, 2, 1.0)[1])"
"#;
    let file = ostinato_lang::parse(range).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "list".to_owned(),
        ostinato_lang::builtin_module("list").expect("list"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], serde_json::json!([0, 1]));
    assert_eq!(json["b"], true);
    assert_eq!(json["c"], "1");

    let cycle = r#"
package demo
params: {a: params.a}
"#;
    let file = ostinato_lang::parse(cycle).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn params_resolution_handles_nested_groups_lets_and_fields() {
    let nested = r#"
package demo
params: {db: {host: string | *"h", url: "http://\(params.db.host)/"}}
"#;
    let file = ostinato_lang::parse(nested).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("nested params");
    assert_eq!(
        value.to_json().expect("json")["params"]["db"]["url"],
        "http://h/"
    );

    let outer_let = r#"
package demo
let base = params.host
params: {host: string | *"h", url: "\(base)/"}
"#;
    let file = ostinato_lang::parse(outer_let).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("outer let params");
    assert_eq!(value.to_json().expect("json")["params"]["url"], "h/");

    let outer_field = r#"
package demo
p: params.a
params: {a: int | *1, b: p}
"#;
    let file = ostinato_lang::parse(outer_field).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("outer field params");
    assert_eq!(value.to_json().expect("json")["params"]["b"], 1);

    let quoted = r#"
package demo
params: {"a": 1, b: params.a}
"#;
    let file = ostinato_lang::parse(quoted).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("quoted params");
    assert_eq!(value.to_json().expect("json")["params"]["b"], 1);
}

#[test]
fn params_optional_and_shadowing_rules_hold() {
    let optional = r#"
package demo
params: {a?: int | *3, b: params.a}
"#;
    let file = ostinato_lang::parse(optional).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let shadow = r#"
package demo
params: {params: {a: 1}, a: 2, b: params.a}
"#;
    let file = ostinato_lang::parse(shadow).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("shadowed params");
    assert_eq!(value.to_json().expect("json")["params"]["b"], 1);
}

#[test]
fn closed_definitions_stay_closed_through_params() {
    let closed = r#"
package demo
#P: {a: 1}
params: #P & {c: 2}
"#;
    let file = ostinato_lang::parse(closed).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let open = r#"
package demo
#P: {a: 1, ...}
params: #P & {c: 2}
"#;
    let file = ostinato_lang::parse(open).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("open params");
    assert_eq!(value.to_json().expect("json")["params"]["c"], 2);
}

#[test]
fn re2_quantifier_grammar_matches_cue() {
    let valid = r#"
package demo
import "regexp"
a: regexp.Valid("a*?")
b: regexp.Valid("a{2}?")
c: regexp.Valid("a{}")
d: regexp.Valid("a{,3}")
e: regexp.Valid("a{1001")
f: regexp.Valid("\\{1001\\}")
"#;
    let file = ostinato_lang::parse(valid).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "regexp".to_owned(),
        ostinato_lang::builtin_module("regexp").expect("regexp"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    for key in ["a", "b", "c", "d", "e", "f"] {
        assert_eq!(json[key], true, "{key}");
    }

    for source in [
        "a: regexp.Valid(\"a**\")",
        "a: regexp.Valid(\"a++\")",
        "a: regexp.Valid(\"a???\")",
        "a: regexp.Valid(\"a{2}{3}\")",
        "a: regexp.Valid(\"a{2}??\")",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"regexp\"\n{source}\n"))
            .expect("parse");
        assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
    }
}

#[test]
fn params_paths_resolve_against_the_params_root() {
    let nested = r#"
package demo
params: {a: {x: 99, inner: {x: 1, y: params.a.x}}}
"#;
    let file = ostinato_lang::parse(nested).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("nested path");
    assert_eq!(
        value.to_json().expect("json")["params"]["a"]["inner"]["y"],
        99
    );

    let parent = r#"
package demo
params: {server: {tls: {port: 443, url: params.server.tls.port}}}
"#;
    let file = ostinato_lang::parse(parent).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("parent path");
    assert_eq!(
        value.to_json().expect("json")["params"]["server"]["tls"]["url"],
        443
    );

    let outer_let = r#"
package demo
let h = params.db.host
params: {db: {host: string | *"h", url: "\(h)/"}}
"#;
    let file = ostinato_lang::parse(outer_let).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("outer let nested");
    assert_eq!(value.to_json().expect("json")["params"]["db"]["url"], "h/");

    let missing = r#"
package demo
params: {a: {b: {c: 1, d: params.a.c}}}
"#;
    let file = ostinato_lang::parse(missing).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn params_dynamic_fields_hidden_definitions_and_quoted_selectors() {
    let dynamic = r#"
package demo
params: {for k in ["x"] { "\(k)": 1 }}
workflow: {name: "\(params.x)", steps: {s: {kind: "noop"}}}
"#;
    let file = ostinato_lang::parse(dynamic).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("dynamic params");
    assert_eq!(value.to_json().expect("json")["workflow"]["name"], "1");

    let locals = r#"
package demo
#P: {a: int}
params: #P & {#Q: {b: 2}, a: #Q.b}
"#;
    let file = ostinato_lang::parse(locals).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("local definition in params");
    assert_eq!(value.to_json().expect("json")["params"]["a"], 2);

    let hidden = r#"
package demo
#P: {a: 1}
params: #P & {_b: 2}
"#;
    let file = ostinato_lang::parse(hidden).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_ok());

    let quoted = r#"
package demo
params: {"a-b": 1, c: params."a-b", d: params["a-b"]}
"#;
    let file = ostinato_lang::parse(quoted).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("quoted selectors");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["c"], 1);
    assert_eq!(json["params"]["d"], 1);
}

#[test]
fn closed_definitions_apply_to_nested_dynamic_and_let_params() {
    let nested = r#"
package demo
#P: {n: {a: 1}}
params: #P & {n: {b: 2}}
"#;
    let file = ostinato_lang::parse(nested).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let comprehension = r#"
package demo
#P: {a: 1}
params: #P & {for k in ["c"] { "\(k)": 2 }}
"#;
    let file = ostinato_lang::parse(comprehension).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());

    let let_bound = r#"
package demo
#P: {a: 1}
let Q = #P
params: Q & {c: 2}
"#;
    let file = ostinato_lang::parse(let_bound).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn closed_definitions_are_checked_inside_referenced_bodies() {
    for source in [
        "#P: {a: 1}\n#Q: #P & {b: 2}\nparams: #Q",
        "#P: {a: 1}\nlet Q = #P & {b: 2}\nparams: Q",
        "#P: {a: 1}\n#Q: #P & {b: 2}\n#R: #Q\nparams: #R",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "closed body must be enforced: {source}"
        );
    }
}

#[test]
fn hidden_fields_are_distinct_from_quoted_underscore_fields() {
    let hidden = r#"
package demo
#P: {a: int}
params: #P & {_b: 2, a: 3}
"#;
    let file = ostinato_lang::parse(hidden).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("hidden field is admitted");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["a"], 3);
    assert!(
        json["params"].get("_b").is_none(),
        "hidden field must not export"
    );

    let quoted = r#"
package demo
#P: {a: 1}
params: #P & {"_x": 2}
"#;
    let file = ostinato_lang::parse(quoted).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "quoted _x is a regular field"
    );

    let generated = r#"
package demo
#P: {a: 1}
params: #P & {for k in ["_x"] { "\(k)": 2 }}
"#;
    let file = ostinato_lang::parse(generated).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "dynamic _x is a regular field"
    );

    let exported = r#"
package demo
params: {_hidden: 1, "_quoted": 2, a: 3}
"#;
    let file = ostinato_lang::parse(exported).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert!(json["params"].get("_hidden").is_none());
    assert_eq!(json["params"]["_quoted"], 2);
}

#[test]
fn params_cache_distinguishes_dotted_labels_from_paths() {
    let flat = r#"
package demo
params: {"a.b": 1, a: {b: 2}, x: params."a.b", y: params.a.b}
"#;
    let file = ostinato_lang::parse(flat).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["x"], 1);
    assert_eq!(json["params"]["y"], 2);

    let nested = r#"
package demo
params: {"a.b": {c: 1}, a: {b: {c: 2}}, x: params."a.b".c, y: params.a.b.c}
"#;
    let file = ostinato_lang::parse(nested).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["x"], 1);
    assert_eq!(json["params"]["y"], 2);
}

#[test]
fn optional_intermediate_params_fields_are_rejected() {
    for source in [
        "params: {a?: {b: 1}, c: params.a.b}",
        "params: {a?: {b: 1} | *{b: 2}, c: params.a.b}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional intermediate must be rejected: {source}"
        );
    }
}

#[test]
fn dynamic_intermediate_and_repeated_segments_resolve() {
    let dynamic = r#"
package demo
params: {for k in ["x"] {"\(k)": {v: 1}}, y: params.x.v}
"#;
    let file = ostinato_lang::parse(dynamic).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("dynamic intermediate");
    assert_eq!(value.to_json().expect("json")["params"]["y"], 1);

    let repeated = r#"
package demo
params: {a: {a: X, b: params.a.a.b}}
X: {b: 1}
"#;
    let file = ostinato_lang::parse(repeated).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("repeated segment");
    assert_eq!(value.to_json().expect("json")["params"]["a"]["b"], 1);
}

#[test]
fn closedness_through_let_mediated_definition_bodies() {
    for source in [
        "#P: {a: 1}\nlet L = #P & {b: 2}\n#Q: L\nparams: #Q",
        "#P: {a: 1}\nlet L = #P\n#Q: L & {b: 2}\nparams: #Q",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "let-mediated closedness must be enforced: {source}"
        );
    }
}

#[test]
fn root_params_patterns_constrain_fields() {
    for source in [
        "params: {[string]: int, a: \"x\"}",
        "params: {[=~\"^x\"]: int, x1: \"bad\"}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "root params pattern must constrain: {source}"
        );
    }

    let hidden = r#"
package demo
params: {[string]: int, _h: "x", a: 1}
"#;
    let file = ostinato_lang::parse(hidden).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("patterns do not match hidden fields");
    assert_eq!(value.to_json().expect("json")["params"]["a"], 1);
}

#[test]
fn nested_params_groups_see_lexical_siblings_through_paths() {
    let source = r#"
package demo
params: {
    host: "example.com"
    db: {url: "http://\(host)/api"}
}
name: params.db.url
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("lexical sibling");
    assert_eq!(
        value.to_json().expect("json")["name"],
        "http://example.com/api"
    );
}

#[test]
fn hidden_fields_are_evaluated_selected_and_conflict_checked() {
    let selected = r#"
package demo
#P: {_h: 3, a: 1}
v: #P._h
"#;
    let file = ostinato_lang::parse(selected).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("hidden select");
    assert_eq!(value.to_json().expect("json")["v"], 3);

    for source in [
        "#P: {_h: int, a: 1}\nparams: #P & {_h: \"x\", a: 1}",
        "params: {_h: 1, _h: 2}",
        "params: {_h: 1 + \"x\"}",
        "params: {q: {_h: 1, _h: 2}}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "hidden field must be evaluated: {source}"
        );
    }

    let alias = r#"
package demo
params: {X=_h: 3, v: X}
"#;
    let file = ostinato_lang::parse(alias).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("hidden alias");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 3);

    let literal = "package demo\nparams: {\"\u{3}a\": 1, a: 2}\n";
    let file = ostinato_lang::parse(literal).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("literal internal-prefix label");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["\u{3}a"], 1);
    assert_eq!(json["params"]["a"], 2);
}

#[test]
fn hidden_fields_are_invisible_to_value_operations() {
    let source = r#"
package demo
params: {
    count: len({_h: 1, a: 2})
    equal: {_h: 1, a: 2} == {a: 2}
    mapped: {for k, v in {_h: 1, a: 2} {"\(k)": v}}
    listed: [for k, _ in {_h: 1, a: 2} {k}]
}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["count"], 1);
    assert_eq!(json["params"]["equal"], true);
    assert_eq!(json["params"]["mapped"], serde_json::json!({"a": 2}));
    assert_eq!(json["params"]["listed"], serde_json::json!(["a"]));
}

#[test]
fn quoted_and_index_selectors_do_not_reach_hidden_fields() {
    for source in [
        "#P: {_h: 3, a: 1}\nx: #P.\"_h\"",
        "#P: {_h: 3, a: 1}\nx: #P[\"_h\"]",
        "#P: {_h: 3, a: 1}\nparams: {x: params._h, a: 1}",
    ] {
        if source.starts_with("#P: {_h: 3, a: 1}\nparams") {
            let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
            assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
            continue;
        }
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(ostinato_lang::eval_file(&file).is_err(), "{source}");
    }

    let identifier = "#P: {_h: 3, a: 1}\nx: #P._h\n";
    let file = ostinato_lang::parse(&format!("package demo\n{identifier}")).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("identifier selector");
    assert_eq!(value.to_json().expect("json")["x"], 3);

    let control = "package demo\n#P: {\"\u{3}a\": 1, a: 2}\nx: #P.\"\u{3}a\"\n";
    let file = ostinato_lang::parse(control).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("escaped quoted selector");
    assert_eq!(value.to_json().expect("json")["x"], 1);
}

#[test]
fn control_prefix_labels_match_patterns() {
    let source = "package demo\n#P: {[=~\"^\u{3}\"]: int}\nparams: #P & {\"\u{3}a\": 1}\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("pattern uses the displayed label");
    assert_eq!(value.to_json().expect("json")["params"]["\u{3}a"], 1);
}

#[test]
fn identifier_selectors_prefer_hidden_over_quoted_twins() {
    let source = r#"
package demo
#P: {_h: 1, "_h": 2}
def: #P._h
params: {_h: 1, "_h": 2, v: params._h, X=_h: 1, xalias: X}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("identifier selectors");
    let json = value.to_json().expect("json");
    assert_eq!(json["def"], 1);
    assert_eq!(json["params"]["v"], 1);
    assert_eq!(json["params"]["xalias"], 1);

    let only_quoted = r#"
package demo
#P: {"_h": 2, a: 3}
def: #P._h
params: {"_h": 2, a: 3, v: params._h}
"#;
    let file = ostinato_lang::parse(only_quoted).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "identifier select must not fall back to a quoted _h"
    );
}

#[test]
fn comprehension_labels_use_displayed_names_for_control_prefixes() {
    let source = "package demo\nsrc: {\"\u{3}a\": 1}\nkeys: [for k, _ in src {k}]\nmapped: {for k, v in src {\"\\(k)\": v}}\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["keys"], serde_json::json!(["\u{3}a"]));
    assert_eq!(json["mapped"]["\u{3}a"], 1);
}

#[test]
fn generated_params_fields_respect_hidden_quoting_namespaces() {
    let generated_quoted = r#"
package demo
params: {for k in ["_h"] {"\(k)": 2}}
v: params._h
"#;
    let file = ostinato_lang::parse(generated_quoted).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "identifier must not see a generated quoted _h"
    );

    let generated_hidden = r#"
package demo
params: {for k in ["x"] {_h: 1}}
v: params._h
"#;
    let file = ostinato_lang::parse(generated_hidden).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated hidden");
    assert_eq!(value.to_json().expect("json")["v"], 1);

    let mixed = r#"
package demo
params: {_h: 1, for k in ["_h"] {"\(k)": 2}}
a: params._h
q: params."_h"
"#;
    let file = ostinato_lang::parse(mixed).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("mixed namespaces");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["q"], 2);

    let conflict = r#"
package demo
params: {_h: 1, for k in ["x"] {_h: 2}}
v: params._h
"#;
    let file = ostinato_lang::parse(conflict).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "generated hidden fields must unify with declared hidden fields"
    );
}

#[test]
fn aliases_to_control_prefix_quoted_labels_resolve() {
    let source = "package demo\ns: {A=\"\u{3}a\": 1, v: A}\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("alias to control label");
    assert_eq!(value.to_json().expect("json")["s"]["v"], 1);
}

#[test]
fn definition_selectors_and_quoted_hash_fields_in_params_paths() {
    let definition = r##"
package demo
#P: {#D: 1, "#D": 2}
params: {g: #P, v: params.g.#D}
"##;
    let file = ostinato_lang::parse(definition).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("definition selector");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 1);

    let single = r##"
package demo
#P: {#D: 1}
params: {g: #P, v: params.g.#D}
"##;
    let file = ostinato_lang::parse(single).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("definition-only selector");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 1);

    let quoted_only = r##"
package demo
params: {"#D": 2, v: params.#D}
"##;
    let file = ostinato_lang::parse(quoted_only).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "identifier #D must not reach a quoted #D field"
    );
}

#[test]
fn generated_quoted_hash_and_control_labels_are_selectable() {
    let hash = r##"
package demo
params: {#D: 1, for k in ["#D"] {"\(k)": 2}, v: params."#D"}
"##;
    let file = ostinato_lang::parse(hash).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated quote");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 2);

    let control =
        "package demo\nparams: {for k in [\"\u{3}a\"] {\"\\(k)\": 1}, v: params.\"\u{3}a\"}\n";
    let file = ostinato_lang::parse(control).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated control");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 1);

    let escaped =
        "package demo\nparams: {for k in [\"\u{4}a\"] {\"\\(k)\": 1}, v: params.\"\u{4}a\"}\n";
    let file = ostinato_lang::parse(escaped).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated escaped");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 1);
}

#[test]
fn quoted_hash_labels_are_selectable_from_struct_values() {
    let source = r##"
package demo
src: {"#D": 1, for k in ["#E"] {"\(k)": 2}}
a: src."#D"
b: src."#E"
"##;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("quoted hash select");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], 1);
    assert_eq!(json["b"], 2);

    let unquoted = r##"
package demo
src: {"#D": 1}
a: src.#D
"##;
    let file = ostinato_lang::parse(unquoted).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn sentinel_prefixed_quoted_labels_select_their_escaped_storage() {
    let hidden_sentinel = "package demo\nsrc: {_h: 7, \"\u{3}_h\": 8}\nq: src.\"\u{3}_h\"\n";
    let file = ostinato_lang::parse(hidden_sentinel).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("sentinel hidden label");
    assert_eq!(value.to_json().expect("json")["q"], 8);

    let non_definition = "package demo\nparams: {src: {\"\u{1}#D\": 5, \"#D\": 6}}\n";
    let file = ostinato_lang::parse(non_definition).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("non-definition sentinel label");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["src"]["\u{1}#D"], 5);
    assert_eq!(json["params"]["src"]["#D"], 6);
}

#[test]
fn imported_packages_drop_package_hidden_fields() {
    let lib = r#"
package lib
#P: {_secret: string, kind: "exec"}
h: {_h: 3, a: 1}
"#;
    let file = ostinato_lang::parse(lib).expect("parse");
    let value =
        ostinato_lang::eval_file_with_imports_mode(&file, &BTreeMap::new(), &BTreeMap::new(), true)
            .expect("evaluate lib");
    let stripped = ostinato_lang::strip_hidden(value);
    let ostinato_lang::Value::Definition {
        fields,
        definitions,
        ..
    } = stripped
    else {
        return;
    };
    let params = definitions
        .get("#P")
        .or_else(|| fields.get("#P"))
        .expect("definition");
    let json = params.to_json().expect("json");
    assert!(json.get("_secret").is_none());
}

#[test]
fn imported_hidden_fields_do_not_leak_into_the_importer() {
    let lib = r#"
package lib
#P: {_secret: string, kind: "exec"}
h: {_h: 3, a: 1}
"#;
    let file = ostinato_lang::parse(lib).expect("parse");
    let value =
        ostinato_lang::eval_file_with_imports_mode(&file, &BTreeMap::new(), &BTreeMap::new(), true)
            .expect("evaluate lib");
    let mut imports = BTreeMap::new();
    imports.insert("lib".to_owned(), ostinato_lang::strip_hidden(value));

    let ok = r#"
package main
params: lib.#P & {_secret: 3, kind: "exec"}
"#;
    let file = ostinato_lang::parse(ok).expect("parse");
    let value = ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new())
        .expect("importer hidden is distinct");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["kind"], "exec");
    assert!(json["params"].get("_secret").is_none());

    let leak = r#"
package main
x: lib.h._h
"#;
    let file = ostinato_lang::parse(leak).expect("parse");
    assert!(ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err());
}

#[test]
fn struct_string_indexes_respect_sentinel_namespaces() {
    let mixed = "package demo\ns: {_h: 3, \"\u{3}_h\": 4}\ni: s[\"\u{3}_h\"]\n";
    let file = ostinato_lang::parse(mixed).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("sentinel index");
    assert_eq!(value.to_json().expect("json")["i"], 4);

    let hidden_only = "package demo\ns: {_h: 3}\ni: s[\"\u{3}_h\"]\n";
    let file = ostinato_lang::parse(hidden_only).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "sentinel index must not read hidden storage"
    );

    let params = "package demo\nparams: {s: {_h: 3, \"\u{3}_h\": 4}, i: params.s[\"\u{3}_h\"]}\n";
    let file = ostinato_lang::parse(params).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("params sentinel index");
    assert_eq!(value.to_json().expect("json")["params"]["i"], 4);
}

#[test]
fn generated_hidden_params_are_not_reachable_through_quoted_selectors() {
    for source in [
        "params: {for k in [\"x\"] {_h: 1}, n: params[\"\u{3}_h\"]}",
        "params: {if true {_h: 1}, n: params.\"\u{3}_h\"}",
        "params: {g: {if true {_h: 1}}, n: params.g[\"\u{3}_h\"]}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "generated hidden must not leak: {source}"
        );
    }

    let mixed = "package demo\nparams: {for k in [\"x\"] {_h: 1, \"\u{3}_h\": 2}, n: params[\"\u{3}_h\"]}\n";
    let file = ostinato_lang::parse(mixed).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated escaped label");
    assert_eq!(value.to_json().expect("json")["params"]["n"], 2);
}

#[test]
fn generated_hidden_optionals_do_not_hide_visible_sentinel_labels() {
    let static_visible = "package demo\nparams: {if true {_h?: 1}, \"\u{3}_h\": 2}\n";
    let file = ostinato_lang::parse(static_visible).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("static visible");
    assert_eq!(value.to_json().expect("json")["params"]["\u{3}_h"], 2);

    let generated_visible = "package demo\nparams: {for k in [\"x\"] {_h?: 1, \"\u{3}_h\": 2}, n: params[\"\u{3}_h\"]}\n";
    let file = ostinato_lang::parse(generated_visible).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated visible");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["\u{3}_h"], 2);
    assert_eq!(json["params"]["n"], 2);
}

#[test]
fn generated_hidden_optionals_in_definitions_are_optional() {
    let source = r#"
package demo
#D: {if true {_h?: 1}, a: 2}
params: #D & {a: 2}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated hidden optional");
    assert_eq!(value.to_json().expect("json")["params"]["a"], 2);
}

#[test]
fn materialized_sentinel_fields_stay_required_and_conflict_checked() {
    let source = "package demo\nparams: {\"\u{3}_x\"?: 2, for k in [\"x\"] {\"\u{3}_x\": 2}, for k in [\"x\"] {\"\u{3}_x\"?: 2}}\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("materialized optional");
    assert_eq!(value.to_json().expect("json")["params"]["\u{3}_x"], 2);

    let conflict = "package demo\nparams: {\"\u{3}_x\"?: 2, for k in [\"x\"] {\"\u{3}_x\": 2}, for k in [\"x\"] {\"\u{3}_x\"?: 3}}\n";
    let file = ostinato_lang::parse(conflict).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "conflict must surface"
    );
}

#[test]
fn generated_root_optionals_do_not_hide_definitions() {
    let lib = r##"
package lib
#D: 1
for k in ["x"] {"#D"?: 3}
"##;
    let file = ostinato_lang::parse(lib).expect("parse");
    let value =
        ostinato_lang::eval_file_with_imports_mode(&file, &BTreeMap::new(), &BTreeMap::new(), true)
            .expect("evaluate lib");
    let mut imports = BTreeMap::new();
    imports.insert("lib".to_owned(), ostinato_lang::strip_hidden(value));

    let main = r#"
package main
params: {v: lib.#D}
"#;
    let file = ostinato_lang::parse(main).expect("parse");
    let value = ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new())
        .expect("definition survives generated optional");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 1);
}

#[test]
fn materialized_hidden_fields_do_not_satisfy_quoted_optionals() {
    for source in [
        "params: {if true {_h: 1}, \"\u{3}_h\"?: 2, v: params[\"\u{3}_h\"]}",
        "params: {if true {_h: 1}, \"\u{3}_h\"?: 2, v: params.\"\u{3}_h\"}",
        "params: {g: {if true {_h: 1}, \"\u{3}_h\"?: 2}, v: params.g.\"\u{3}_h\"}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "hidden materialization must not satisfy quoted optional: {source}"
        );
    }
}

#[test]
fn quoted_params_lookup_does_not_return_cached_definitions() {
    for source in [
        "params: {#D: 1, if #D == 1 {a: 1}, q: params[\"#D\"]}",
        "params: {#D: 1, for k in [#D] {a: k}, q: params[\"#D\"]}",
        "params: {#D: 1, k: \"#D\", if #D == 1 {a: 1}, q: params[k]}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "quoted lookup must not return a definition: {source}"
        );
    }

    let generated = r##"
package demo
params: {#D: 1, for k in ["#D"] {"\(k)": 2}, v: params."#D"}
"##;
    let file = ostinato_lang::parse(generated).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("generated quoted label");
    assert_eq!(value.to_json().expect("json")["params"]["v"], 2);
}

#[test]
fn overrides_keep_generated_optional_constraints() {
    let source = r#"
package demo
params: {a: string @tag(a), for k in ["x"] {a?: "x"}}
workflow: {name: "\(params.a)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("a".to_owned(), "x".to_owned())]),
    )
    .expect("matching override");
    assert_eq!(compiled.spec.name, "x");

    let error = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("a".to_owned(), "y".to_owned())]),
    )
    .expect_err("conflicting override");
    assert!(error.to_string().contains("conflict"), "{error}");

    let conditional = r#"
package demo
params: {a: int @tag(a, type=int), if true {a?: 2}}
workflow: {name: "n", steps: {s: {kind: "noop"}}}
"#;
    assert!(
        ostinato_lang::compile_with_params(
            conditional,
            &BTreeMap::from([("a".to_owned(), "3".to_owned())]),
        )
        .is_err()
    );
}

#[test]
fn params_comprehensions_can_read_siblings_without_false_cycles() {
    let cases = [
        "params: {a: 1, b: 2, for k in [params.a] {c: k}}",
        "params: {a: 1, if params.a == 1 {c: 2}}",
        "params: {env: string | *\"dev\", if params.env == \"prod\" {replicas: int | *3}}",
        "params: {db: {host: string | *\"h\"}, if params.db.host == \"h\" {port: 1}}",
    ];
    for source in cases {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect("params sibling inside comprehension");
    }

    for source in [
        "params: {a: params.a}",
        "params: {a: params.b, b: params.a}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "true cycle: {source}"
        );
    }
}

#[test]
fn overrides_are_visible_to_params_comprehensions() {
    let conditional = r#"
package demo
params: {a: int @tag(a, type=int), b: int, if a == 2 {b: 5}}
workflow: {name: "\(params.b)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        conditional,
        &BTreeMap::from([("a".to_owned(), "2".to_owned())]),
    )
    .expect("conditional override");
    assert_eq!(compiled.spec.name, "5");

    let looped = r#"
package demo
params: {a: int @tag(a, type=int), b: int, for k in [a] {b?: k}}
workflow: {name: "\(params.b)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        looped,
        &BTreeMap::from([("a".to_owned(), "2".to_owned())]),
    )
    .expect("loop override");
    assert_eq!(compiled.spec.name, "2");
}

#[test]
fn nested_params_groups_and_generated_chains_avoid_false_cycles() {
    let cases = [
        "params: {db: {host: \"h\", if params.db.host == \"h\" {port: 1}}}",
        "params: {a: 1, for k in [params.a] {b: k}, for j in [params.b] {c: j}}",
        "params: {let x = 1, g: {b: x}}",
        "params: {a: 1, b: 2, for k in [params.a] {c: k}, if params.c == 1 {d: 2}}",
    ];
    for source in cases {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect("nested params sibling");
    }
}

#[test]
fn nested_params_groups_see_outer_overrides() {
    let source = r#"
package demo
params: {a: int @tag(a, type=int), g: {for k in [a] {b: k}}}
workflow: {name: "\(params.g.b)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        source,
        &BTreeMap::from([("a".to_owned(), "2".to_owned())]),
    )
    .expect("outer override in group");
    assert_eq!(compiled.spec.name, "2");
}

#[test]
fn params_index_paths_resolve_through_nested_groups() {
    let cases = [
        "params: {db: {host: \"h\", port: params.db[\"host\"]}}",
        "params: {g: {a: 1, v: params[\"g\"].a}}",
        "params: {g: {\"a.b\": 1, v: params[\"g\"][\"a.b\"]}}",
        "params: {\"g.h\": {a: 1, b: params[\"g.h\"].a}}",
        "params: {db: {x: 1, for k in [\"x\"] {b: params.db[k]}}}",
        "params: {db: {inner: {x: 1, for k in [\"x\"] {b: params.db.inner[k]}}}}",
    ];
    for source in cases {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect("index path");
    }

    let definition = r#"
package demo
#P: {db: {host: "h", port: params.db["host"]}}
params: #P
"#;
    let file = ostinato_lang::parse(definition).expect("parse");
    ostinato_lang::eval_file(&file).expect("definition index path");
}

#[test]
fn definition_backed_params_see_overrides_in_dependents() {
    let dependent = r#"
package demo
#P: {name: string @tag(name), greeting: "Hello \(name)"}
params: #P
workflow: {name: "\(params.greeting)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        dependent,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("definition dependent override");
    assert_eq!(compiled.spec.name, "Hello Alice");

    let defaulted = r#"
package demo
#P: {name: string | *"world" @tag(name), greeting: "Hello \(name)"}
params: #P
workflow: {name: "\(params.greeting)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        defaulted,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("definition default override");
    assert_eq!(compiled.spec.name, "Hello Alice");

    let conditional = r#"
package demo
#P: {name: string @tag(name), if name == "Alice" {g: 1}}
params: #P
workflow: {name: "\(params.g)", steps: {s: {kind: "noop"}}}
"#;
    let compiled = ostinato_lang::compile_with_params(
        conditional,
        &BTreeMap::from([("name".to_owned(), "Alice".to_owned())]),
    )
    .expect("definition conditional override");
    assert_eq!(compiled.spec.name, "1");
}

#[test]
fn params_dependency_chains_resolve_in_comprehensions() {
    let valid = [
        "params: {a: 1, b: params.a, if params.b == 1 {c: 2}}",
        "params: {host: string | *\"h\", url: \"http://\\(params.host)/\", if params.url != \"\" {endpoint: params.url}}",
        "params: {a: 1, b: params.a, for k in [params.b] {c: k}}",
        "params: {a: 1, if params.a == 1 {db: {x: 2, for k in [\"x\"] {v: params.db[k]}}}}",
    ];
    for source in valid {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect("dependency chain");
    }

    let cycle = "params: {a: params.b, b: params.a}";
    let file = ostinato_lang::parse(&format!("package demo\n{cycle}\n")).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "true cycle: {cycle}"
    );
}

#[test]
fn chained_dynamic_groups_resolve_sibling_values() {
    let source = r#"
package demo
params: {
    a: {b: {c: {x: 7, for k in ["x"] {v: params.a.b.c[k]}}}}
    w: params.a.b.c.v
}
workflow: {name: "\(params.w)", steps: {s: {kind: "noop"}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("chained dynamic group");
    assert_eq!(value.to_json().expect("json")["workflow"]["name"], "7");
}

#[test]
fn absolute_params_paths_do_not_shadow_into_nested_groups() {
    let source = "params: {z: params.b.c, b: {z: 2, c: params.z}}";
    let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "true cycle must error"
    );
}

#[test]
fn dynamic_index_then_selector_resolves_params_paths() {
    let source = r#"
package demo
params: {k: "g", g: {x: 1, y: params[k].x}}
workflow: {name: "\(params.g.y)", steps: {s: {kind: "noop"}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("dynamic index selector");
    assert_eq!(value.to_json().expect("json")["workflow"]["name"], "1");
}

#[test]
fn params_items_can_depend_on_later_generated_siblings() {
    let cases = [
        "params: {if params.a == 1 {b: 2}, if true {a: 1}}",
        "params: {if params.a == 1 {b: 2}, for k in [1] {a: k}}",
        "params: {for k in [params.a] {b: k}, for j in [1] {a: j}}",
    ];
    for source in cases {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        ostinato_lang::eval_file(&file).expect("deferred generated sibling");
    }
}

#[test]
fn absolute_params_references_inside_generated_groups_resolve() {
    let source = r#"
package demo
params: {x: 1, if true {g: {x: 2, y: params.x}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("absolute path in group");
    assert_eq!(value.to_json().expect("json")["params"]["g"]["y"], 1);

    let looped = r#"
package demo
params: {x: 1, for k in ["g"] {"\(k)": {x: 2, y: params.x}}}
"#;
    let file = ostinato_lang::parse(looped).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("absolute path in for group");
    assert_eq!(value.to_json().expect("json")["params"]["g"]["y"], 1);
}

#[test]
fn cross_conditional_params_groups_share_materialized_values() {
    let source = r#"
package demo
params: {if true {g: {x: 1, y: params.g.x}}, if true {h: {z: params.g.y}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("cross conditional groups");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["g"]["y"], 1);
    assert_eq!(json["params"]["h"]["z"], 1);
}

#[test]
fn recursive_definitions_materialize_finite_values() {
    let source = r#"
package demo
#Tree: {value: int, children?: [...#Tree]}
#Node: {value: int, next?: #Node}
#MutualA: {name: string, b?: #MutualB}
#MutualB: {name: string, a?: #MutualA}
tree: #Tree & {value: 1, children: [{value: 2}, {value: 3, children: [{value: 4}]}]}
node: #Node & {value: 1, next: {value: 2, next: {value: 3}}}
mutual: #MutualA & {name: "a", b: {name: "b", a: {name: "c"}}}
tree2: #Tree & tree
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("recursive definitions");
    let json = value.to_json().expect("json");
    assert_eq!(json["tree"]["children"][1]["children"][0]["value"], 4);
    assert_eq!(json["node"]["next"]["next"]["value"], 3);
    assert_eq!(json["mutual"]["b"]["a"]["name"], "c");
    assert_eq!(json["tree2"], json["tree"]);
}

#[test]
fn recursive_definitions_apply_nested_constraints() {
    let valid = r#"
package demo
#Tree: {value: int | *0, children?: [...#Tree]}
tree: #Tree & {children: [{value: 2}]}
"#;
    let file = ostinato_lang::parse(valid).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("defaults and recursion");
    let json = value.to_json().expect("json");
    assert_eq!(json["tree"]["value"], 0);
    assert_eq!(json["tree"]["children"][0]["value"], 2);

    let ill_typed = "#Tree: {value: int, children?: [...#Tree]}\ntree: #Tree & {value: 1, children: [{value: \"x\"}]}";
    let file = ostinato_lang::parse(&format!("package demo\n{ill_typed}\n")).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "nested recursive constraints must be enforced"
    );

    let infinite = "#Node: {value: int, next: #Node}\nnode: #Node & {value: 1, next: {}}";
    let file = ostinato_lang::parse(&format!("package demo\n{infinite}\n")).expect("parse");
    if let Ok(value) = ostinato_lang::eval_file(&file) {
        assert!(
            value.to_json().is_none(),
            "structurally infinite definitions must stay incomplete"
        );
    }
}

#[test]
fn recursive_definitions_fill_defaults_at_every_depth() {
    let source = r#"
package demo
#Node: {kind: string | *"leaf", value: int, children?: [...#Node]}
nodes: #Node & {kind: "root", value: 1, children: [{value: 2, children: [{value: 3}]}]}
list: [...{v: int, tags: [...string]}]
list: [{v: 2}]
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("defaults at depth");
    let json = value.to_json().expect("json");
    assert_eq!(json["nodes"]["kind"], "root");
    assert_eq!(json["nodes"]["children"][0]["kind"], "leaf");
    assert_eq!(json["nodes"]["children"][0]["children"][0]["kind"], "leaf");
    assert_eq!(json["list"][0]["tags"], serde_json::json!([]));

    let missing = r#"
package demo
#Node: {kind: string, value: int, children?: [...#Node]}
nodes: #Node & {kind: "root", value: 1, children: [{value: 2}]}
"#;
    let file = ostinato_lang::parse(missing).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("deferred structural cycle");
    assert!(
        value.to_json().is_none(),
        "depth >= 2 missing required fields must stay incomplete"
    );

    for source in [
        "#N: {value: int, next: #N} & {value: 1, next: {}}\nn: #N\n",
        "#T: {value: int, next?: #T} & {value: 1, next: {}}\nn: #T\n",
        "#T: {children: [...#T]} & {children: [{}]}\nn: #T\n",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}")).expect("parse");
        if let Ok(value) = ostinato_lang::eval_file(&file) {
            assert!(
                value.to_json().is_none(),
                "structural cycles must stay incomplete: {source}"
            );
        }
    }
}

#[test]
fn deep_values_compare_and_materialize_beyond_128_levels() {
    fn nested(depth: usize) -> String {
        let mut value = "leaf: true".to_owned();
        for _ in 0..depth {
            value = format!("{{inner: {value}}}");
        }
        value
    }
    let source = format!(
        "package demo\na: {}\nb: {}\nparams: {{eq: a == b, neq: a != b}}\n",
        nested(200),
        nested(200)
    );
    let file = ostinato_lang::parse(&source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("deep equality");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["eq"], true);
    assert_eq!(json["params"]["neq"], false);

    fn tree(depth: usize) -> String {
        let mut value = format!("{{value: {depth}}}");
        for index in (1..depth).rev() {
            value = format!("{{value: {index}, children: [{value}]}}");
        }
        value
    }
    let source = format!(
        "package demo\n#Tree: {{value: int, children?: [...#Tree]}}\ntree: #Tree & {}\nparams: {{x: tree, same: tree == tree}}\n",
        tree(300)
    );
    let file = ostinato_lang::parse(&source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("deep recursion");
    let json = value.to_json().expect("deep json");
    assert_eq!(json["params"]["same"], true);
    let mut node = &json["params"]["x"];
    for _ in 0..299 {
        node = &node["children"][0];
    }
    assert_eq!(node["value"], 300);
}

#[test]
fn open_list_prefixes_align_and_enforce_element_constraints() {
    let conflict = r#"
package demo
x: ["a", ...] & [...int] & ["a"]
"#;
    let file = ostinato_lang::parse(conflict).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "prefix elements must satisfy the open-list element constraint"
    );

    let valid = r#"
package demo
#N: {kind: string | *"leaf", value: int, children?: [...#N]}
x: [{value: 1}, ...#N] & [...{value: int}] & [{value: 1}]
y: [1, ...int] & [1, 2, ...int] & [1, 2, 3]
"#;
    let file = ostinato_lang::parse(valid).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("aligned prefixes");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"], serde_json::json!([{"value": 1}]));
    assert_eq!(json["y"], serde_json::json!([1, 2, 3]));

    let tails = r#"
package demo
empty: [...int] & [...string]
prefixed: [1, ...int] & [1, ...string]
prefixed_other: [1, ...int] & [1, 2, ...string]
mixed: [...int] & [1, ...string]
"#;
    let file = ostinato_lang::parse(tails).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("incompatible tails collapse");
    let json = value.to_json().expect("json");
    assert_eq!(json["empty"], serde_json::json!([]));
    assert_eq!(json["prefixed"], serde_json::json!([1]));
    assert_eq!(json["prefixed_other"], serde_json::json!([1, 2]));
    assert_eq!(json["mixed"], serde_json::json!([1]));

    let nested = r#"
package demo
#A: {a: 1}
#B: {b: 2}
nested_empty: [...[1]] & [...[1, 2]]
nested_prefixed: [1, ...[1]] & [1, ...[1, 2]]
nested_struct: [...{a: [1]}] & [...{a: [1, 2]}]
nested_prefix: [...[1, 2, ...int]] & [...[1]]
closed: [...#A] & [...#B]
"#;
    let file = ostinato_lang::parse(nested).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("nested incompatible tails collapse");
    let json = value.to_json().expect("json");
    assert_eq!(json["nested_empty"], serde_json::json!([]));
    assert_eq!(json["nested_prefixed"], serde_json::json!([1]));
    assert_eq!(json["nested_struct"], serde_json::json!([]));
    assert_eq!(json["nested_prefix"], serde_json::json!([]));
    assert_eq!(json["closed"], serde_json::json!([]));

    let positional = r#"
package demo
bad: [1, 2, ...int] & [1, ...string]
"#;
    let file = ostinato_lang::parse(positional).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "prefix-position conflicts must remain errors"
    );
}

#[test]
fn equality_requires_concrete_operands() {
    for source in [
        "a: int\nparams: {eq: a == a}",
        "#N: {value: int, next: #N}\nn: #N\nparams: {eq: n == n}",
        "#N: {value: int, next?: #N}\nn: #N\nparams: {eq: n != n}",
        "params: {eq: [int, ...int] == [int, ...int]}",
        "params: {eq: [>=1, ...int] == [>=1, ...int]}",
        "params: {eq: [int, ...int] == [1]}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "non-concrete equality must be rejected: {source}"
        );
    }

    let concrete = "package demo\na: 1\nb: 1\nparams: {eq: a == b, neq: a != b, same: [1, ...int] == [1], cross: [...int] == [...string], diff: [1, ...int] == [...int]}\n";
    let file = ostinato_lang::parse(concrete).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("concrete equality");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["eq"], true);
    assert_eq!(json["params"]["neq"], false);
    assert_eq!(json["params"]["same"], true);
    assert_eq!(json["params"]["cross"], true);
    assert_eq!(json["params"]["diff"], false);
}

#[test]
fn go_time_layouts_and_helpers_match_cue() {
    let source = r#"
package demo
import "time"
a: time.Parse(time.ANSIC, "Tue Mar  5 06:07:08 2024")
b: time.Parse(time.RFC1123Z, "Tue, 05 Mar 2024 06:07:08 +0200")
c: time.Parse("Mon 2 Jan 2006", "Tue 5 Mar 2024")
d: time.Parse("2006-002", "2024-065")
e: time.Format("2024-03-05", "2006-01-02")
f: time.FormatString("Mon Jan _2 15:04:05 2006", "2024-03-05T06:07:08Z")
g: time.ToUnix("2024-03-05T06:07:08Z")
h: time.ToUnixNano("2024-03-05T06:07:08.000000005Z")
i: time.Split("2024-03-05T06:07:08.12Z")
j: time.FormatDuration(3723000500000)
k: time.Second
l: time.FormatString("15:04 -07:00", "2024-03-05T06:07:08+24:00")
m: time.ParseDuration("0." + ("9"*100) + "s")
n: time.Parse(".999.", ".")
o: time.Parse("15:04:05.", "12:30:00.")
p: time.ParseDuration("0.999999999999999999s")
q: time.ParseDuration("0.99999999999999999s")
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "time".to_owned(),
        ostinato_lang::builtin_module("time").expect("time"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "2024-03-05T06:07:08Z");
    assert_eq!(json["b"], "2024-03-05T04:07:08Z");
    assert_eq!(json["c"], "2024-03-05T00:00:00Z");
    assert_eq!(json["d"], "2024-03-05T00:00:00Z");
    assert_eq!(json["e"], true);
    assert_eq!(json["f"], "Tue Mar  5 06:07:08 2024");
    assert_eq!(json["g"], 1_709_618_828i64);
    assert_eq!(json["h"], 1_709_618_828_000_000_005i64);
    assert_eq!(json["i"]["nanosecond"], 120_000_000);
    assert_eq!(json["j"], "1h2m3.0005s");
    assert_eq!(json["k"], 1_000_000_000i64);
    assert_eq!(json["l"], "06:07 +24:00");
    assert_eq!(json["m"], 1_000_000_000i64);
    assert_eq!(json["n"], "0000-01-01T00:00:00Z");
    assert_eq!(json["o"], "0000-01-01T12:30:00Z");
    assert_eq!(json["p"], 1_000_000_000i64);
    assert_eq!(json["q"], 1_000_000_000i64);

    let fractions = r#"
package demo
import "time"
a: time.Parse("2006-01-02T15:04:05.000Z07:00", "2024-03-05T06:07:08,123Z")
b: time.Parse("2006-01-02T15:04:05.999Z07:00", "2024-03-05T06:07:08.1234567891Z")
"#;
    let file = ostinato_lang::parse(fractions).expect("parse");
    let value = ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new())
        .expect("fraction layouts");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "2024-03-05T06:07:08.123Z");
    assert_eq!(json["b"], "2024-03-05T06:07:08.123456789Z");

    for source in [
        r#"a: time.Parse("Mon", "Xyz")"#,
        r#"a: time.Parse("MST", "PSTX")"#,
        r#"a: time.Parse("MST", "utc")"#,
        r#"a: time.Parse("2006-002 01-02", "2024-065 03-06")"#,
        r#"a: time.Format("😀 5, 2024", "Jan 2, 2006")"#,
        r#"a: time.Parse("2006-01-02T15:04:05.000Z07:00", "2024-03-05T06:07:08.12Z")"#,
        r#"a: time.Parse("2006-01", "2024-13")"#,
        r#"a: time.Parse("2006-01-02", "2024-02-30")"#,
        r#"a: time.Parse("15:04:05", "24:00:00")"#,
        r#"a: time.Parse("15:04:05", "23:59:60")"#,
        r#"a: time.Parse("3:04PM", "13:04PM")"#,
        r#"a: time.Format("2024-3-5", "2006-01-02")"#,
        r#"a: time.Time("2024-03-05 06:07:08")"#,
        r#"a: time.Duration("1h30")"#,
    ] {
        let file = ostinato_lang::parse(&format!("package demo\nimport \"time\"\n{source}\n"))
            .expect("parse");
        assert!(
            ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err(),
            "invalid time input must fail: {source}"
        );
    }
}

#[test]
fn string_and_bytes_repetition_match_cue() {
    let source = r#"
package demo
a: "ab" * 3
b: 2 * "xy"
c: "z" * 0
d: 'ab' * 2
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("repeat");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "ababab");
    assert_eq!(json["b"], "xyxy");
    assert_eq!(json["c"], "");
    assert_eq!(json["d"], "YWJhYg==");

    let negative = "package demo\na: \"x\" * -1\n";
    let file = ostinato_lang::parse(negative).expect("parse");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
#[allow(clippy::approx_constant)]
fn math_integral_results_keep_the_cue_kind() {
    let source = r#"
package demo
import "math"
sqrt: math.Sqrt(4)
sqrt_float: math.Sqrt(4) & float
sqrt_two: math.Sqrt(2)
pow: math.Pow(2, 3)
exp: math.Exp(0)
log: math.Log(1)
floor: math.Floor(2.7)
ceil: math.Ceil(2.1)
round: math.Round(2.5)
trunc: math.Trunc(2.9)
abs: math.Abs(-2)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "math".to_owned(),
        ostinato_lang::builtin_module("math").expect("math"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["sqrt"], 2);
    assert_eq!(json["sqrt_float"], 2);
    assert_eq!(json["sqrt_two"], 1.4142135623730951);
    assert_eq!(json["pow"], 8);
    assert_eq!(json["exp"], 1);
    assert_eq!(json["log"], 0);
    assert_eq!(json["floor"], 2);
    assert_eq!(json["ceil"], 3);
    assert_eq!(json["round"], 3);
    assert_eq!(json["trunc"], 2);
    assert_eq!(json["abs"], 2);

    let invalid = "package demo\nimport \"math\"\nbad: math.Sqrt(4) & int\n";
    let file = ostinato_lang::parse(invalid).expect("parse");
    assert!(
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err(),
        "Sqrt must stay a float"
    );

    let zero_power = "package demo\nimport \"math\"\nbad: math.Pow(0, 0)\n";
    let file = ostinato_lang::parse(zero_power).expect("parse");
    assert!(
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).is_err(),
        "Pow(0,0) is invalid in CUE"
    );
}

#[test]
fn multiline_bytes_literals_dedent_and_interpolate() {
    let source = r#"
package demo
plain: '''
line1
line2
'''
interp: '''
a\(1+2)b
'''
indented: '''
    line1
      line2
    '''
blank: '''
line1

line2
'''
empty: '''
'''
unicode: '''
日本語
'''
"#;
    let file = ostinato_lang::parse(source).expect("parse multi-line bytes");
    let value = ostinato_lang::eval_file(&file).expect("eval multi-line bytes");
    let json = value.to_json().expect("json");
    assert_eq!(json["plain"], "bGluZTEKbGluZTI=");
    assert_eq!(json["interp"], "YTNi");
    assert_eq!(json["indented"], "bGluZTEKICBsaW5lMg==");
    assert_eq!(json["blank"], "bGluZTEKCmxpbmUy");
    assert_eq!(json["empty"], "");
    assert_eq!(json["unicode"], "5pel5pys6Kqe");

    for source in [
        "package demo\na: '''abc\n'''\n",
        "package demo\na: '''\n  one\ntwo\n  '''\n",
        "package demo\na: '''\none\ntwo'''\n",
        "package demo\na: 'one\ntwo'\n",
    ] {
        assert!(
            ostinato_lang::parse(source).is_err(),
            "invalid multi-line bytes must fail: {source:?}"
        );
    }
}

#[test]
fn bytes_interpolation_formats_scalars_and_rejects_incomplete() {
    let source = r#"
package demo
b: 'raw'
s: "text"
n: 42
f: 1.50
flag: true
int_text: 'a\(n)b'
string_text: 'a\(s)b'
bytes_text: 'a\(b)b'
bool_text: 'a\(flag)b'
float_text: 'a\(f)b'
nested: 'a\((1+2)*3)b'
"#;
    let file = ostinato_lang::parse(source).expect("parse interpolated bytes");
    let value = ostinato_lang::eval_file(&file).expect("eval interpolated bytes");
    let json = value.to_json().expect("json");
    assert_eq!(json["int_text"], "YTQyYg==");
    assert_eq!(json["string_text"], "YXRleHRi");
    assert_eq!(json["bytes_text"], "YXJhd2I=");
    assert_eq!(json["bool_text"], "YXRydWVi");
    assert_eq!(json["float_text"], "YTEuNTBi");
    assert_eq!(json["nested"], "YTli");

    for source in [
        "package demo\na: 'x\\(null)y'\n",
        "package demo\na: 'x\\([1])y'\n",
        "package demo\na: 'x\\({a: 1})y'\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "non-scalar interpolation must fail: {source:?}"
        );
    }
}

#[test]
fn multiline_bytes_dedent_handles_cr_and_interpolation_lines() {
    let source = "package demo\na: '''\n  x\\(1 +\n2)y\n  '''\nb: '''\n  a\rb\n  '''\nc: '''\r\n  a\r\n  \r\n  b\r\n  '''\nd: '''\n  a\\rb\n  '''\n";
    let file = ostinato_lang::parse(source).expect("parse CR and interpolation lines");
    let value = ostinato_lang::eval_file(&file).expect("eval CR and interpolation lines");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "eDN5");
    assert_eq!(json["b"], "YWI=");
    assert_eq!(json["c"], "YQoKYg==");
    assert_eq!(json["d"], "YQ1i");

    // A CRLF blank line contributes its CR to the leading whitespace, so a
    // non-empty closing indentation does not match.
    let invalid = "package demo\na: '''\r\n  a\r\n\r\n  b\r\n  '''\n";
    assert!(
        ostinato_lang::parse(invalid).is_err(),
        "CRLF blank lines must not be silently normalized before the dedent check"
    );

    // A line that starts with interpolation at column zero has no literal
    // leading whitespace, so a non-empty closing indentation must not match.
    let invalid = "package demo\na: '''\n\\(1)\n  '''\n";
    assert!(
        ostinato_lang::parse(invalid).is_err(),
        "interpolation at column zero must reject a non-empty closing indentation"
    );
}

#[test]
fn multiline_bytes_escaped_backslash_before_paren_is_literal() {
    let source = "package demo\na: '''\n  x\\\\(1 +\n  2)y\n  '''\n";
    let file = ostinato_lang::parse(source).expect("parse escaped backslash + paren");
    let value = ostinato_lang::eval_file(&file).expect("eval escaped backslash + paren");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "eFwoMSArCjIpeQ==");

    // The continuation line is literal text, so a non-empty closing indent
    // must reject it when the continuation is not indented.
    let invalid = "package demo\na: '''\n  x\\\\(1 +\n2)y\n  '''\n";
    assert!(
        ostinato_lang::parse(invalid).is_err(),
        "unescaped interpolation must not swallow literal continuation lines"
    );
}

#[test]
fn multiline_strings_use_cue_dedent_and_cr_rules() {
    let source = r#"
package demo
basic: """
line1
line2
"""
indented: """
    line1
      line2
    """
triple: """
a\"""b
"""
interp: """
  x \(1 +
2) y
  """
blank: """
line1

line2
"""
unicode: """
日本語
"""
"#;
    let file = ostinato_lang::parse(source).expect("parse multiline strings");
    let value = ostinato_lang::eval_file(&file).expect("eval multiline strings");
    let json = value.to_json().expect("json");
    assert_eq!(json["basic"], "line1\nline2");
    assert_eq!(json["indented"], "line1\n  line2");
    assert_eq!(json["triple"], "a\"\"\"b");
    assert_eq!(json["interp"], "x 3 y");
    assert_eq!(json["blank"], "line1\n\nline2");
    assert_eq!(json["unicode"], "日本語");

    let crlf = "package demo\na: \"\"\"\r\n  x\r\n  y\r\n  \"\"\"\n";
    let file = ostinato_lang::parse(crlf).expect("parse CRLF multiline string");
    let value = ostinato_lang::eval_file(&file).expect("eval CRLF multiline string");
    assert_eq!(value.to_json().expect("json")["a"], "x\ny");

    let raw_cr = "package demo\na: \"\"\"\n  a\rb\n  \"\"\"\n";
    let file = ostinato_lang::parse(raw_cr).expect("parse CR multiline string");
    let value = ostinato_lang::eval_file(&file).expect("eval CR multiline string");
    assert_eq!(value.to_json().expect("json")["a"], "ab");

    let escaped_cr = r#"
package demo
a: """
  a\rb
  """
"#;
    let file = ostinato_lang::parse(escaped_cr).expect("parse escaped CR");
    let value = ostinato_lang::eval_file(&file).expect("eval escaped CR");
    assert_eq!(value.to_json().expect("json")["a"], "a\rb");

    // CUE allows a bare CR in the line-start whitespace, including on the
    // closing delimiter line.
    let cr_prefix = "package demo\na: \"\"\"\n\raaa\n\rbbb\n\r\"\"\"\n";
    let file = ostinato_lang::parse(cr_prefix).expect("parse CR-prefixed string lines");
    let value = ostinato_lang::eval_file(&file).expect("eval CR-prefixed string lines");
    assert_eq!(value.to_json().expect("json")["a"], "aaa\nbbb");

    let cr_prefix_blank = "package demo\na: \"\"\"\n\r\na\n\r\n\"\"\"\n";
    let file = ostinato_lang::parse(cr_prefix_blank).expect("parse CR-prefixed blank lines");
    let value = ostinato_lang::eval_file(&file).expect("eval CR-prefixed blank lines");
    assert_eq!(value.to_json().expect("json")["a"], "\na\n");

    let cr_prefix_bytes = "package demo\na: '''\n\raaa\n\rbbb\n\r'''\n";
    let file = ostinato_lang::parse(cr_prefix_bytes).expect("parse CR-prefixed bytes lines");
    let value = ostinato_lang::eval_file(&file).expect("eval CR-prefixed bytes lines");
    assert_eq!(value.to_json().expect("json")["a"], "YWFhCmJiYg==");

    for source in [
        "package demo\na: \"\"\"\r\n  a\r\n\r\n  b\r\n  \"\"\"\n",
        "package demo\na: \"\"\"\n  a\nb\n  \"\"\"\n",
        "package demo\na: \"\"\"x\n\"\"\"\n",
        "package demo\na: \"one\ntwo\"\n",
        "package demo\na: '''\n  a\\\r(1)b\n  '''\n",
        "package demo\na: \"\"\"\n  a\\\r(1)b\n  \"\"\"\n",
    ] {
        assert!(
            ostinato_lang::parse(source).is_err(),
            "invalid multiline/raw input must fail: {source:?}"
        );
    }
}

const GATEWAY_SOURCE: &str = r#"
package demo

params: {
    fast: bool | *true
}

workflow: {
    name: "gateway"
    steps: {
        test: { kind: "noop" }
        route: {
            kind:  "gateway"
            needs: ["test"]
            gateway: {
                split: "xor"
                branches: {
                    deployFast: "params.fast && steps.test.exitCode == 0"
                    deploySlow: true
                }
                default: "deploySlow"
            }
        }
        deployFast: { kind: "noop" }
        deploySlow: { kind: "noop" }
        merge: {
            kind:  "gateway"
            needs: ["deployFast", "deploySlow"]
            gateway: {join: "or"}
        }
        notify: { kind: "noop", needs: ["merge"] }
    }
}
"#;

fn gateway_step_source(split: &str, branches: &str, default: &str) -> String {
    format!(
        r#"
package demo
params: {{ x: bool | *true }}
workflow: {{
    name: "mode"
    steps: {{
        test: {{ kind: "noop" }}
        route: {{
            kind: "gateway"
            needs: ["test"]
            gateway: {{ split: "{split}", branches: {{ {branches} }}{default} }}
        }}
        a: {{ kind: "noop" }}
        b: {{ kind: "noop" }}
    }}
}}
"#
    )
}

fn compile_error(source: &str) -> String {
    ostinato_lang::compile(source)
        .expect_err("must reject")
        .to_string()
}

#[test]
fn compiles_every_split_and_join_mode() {
    for (mode, branches, default) in [
        ("and", "a: true, b: true", ""),
        ("or", "a: \"params.x\", b: true", ""),
        ("xor", "a: \"params.x\", b: true", ", default: \"b\""),
    ] {
        let source = gateway_step_source(mode, branches, default);
        let compiled = ostinato_lang::compile(&source).expect("compile split");
        let gateway = compiled
            .spec
            .step("route")
            .and_then(|step| step.gateway.as_ref())
            .expect("gateway");
        assert!(gateway.split.is_some(), "split mode {mode}");
    }
    for mode in ["and", "or", "xor"] {
        let source = format!(
            r#"
package demo
workflow: {{
    name: "join"
    steps: {{
        a: {{ kind: "noop" }}
        b: {{ kind: "noop" }}
        merge: {{ kind: "gateway", needs: ["a", "b"], gateway: {{ join: "{mode}" }} }}
    }}
}}
"#
        );
        let compiled = ostinato_lang::compile(&source).expect("compile join");
        assert!(
            compiled
                .spec
                .step("merge")
                .and_then(|step| step.gateway.as_ref())
                .and_then(|gateway| gateway.join)
                .is_some()
        );
    }
}

#[test]
fn gateway_conditions_and_step_status_are_accepted() {
    let compiled = ostinato_lang::compile(GATEWAY_SOURCE).expect("compile");
    let gateway = compiled
        .spec
        .step("route")
        .and_then(|step| step.gateway.as_ref())
        .expect("gateway");
    assert_eq!(gateway.default.as_deref(), Some("deploySlow"));
    assert_eq!(
        gateway.branches["deployFast"].condition.as_deref(),
        Some("params.fast && steps.test.exitCode == 0")
    );
    assert!(gateway.branches["deploySlow"].condition.is_none());
    assert_eq!(
        compiled.spec.step("route").expect("route").kind,
        StepKind::Gateway
    );
}

#[test]
fn gateway_payload_rules_are_rejected_with_full_paths() {
    let cases = [
        (
            r#"a: {kind:"gateway"}
               b: {kind:"noop"}"#,
            "workflow.steps.a.gateway is required",
        ),
        (
            r#"a: {kind:"exec", exec:{cmd:["true"]}, gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "workflow.steps.a.gateway is only allowed",
        ),
        (
            r#"a: {kind:"gateway", needs:["b"], gateway:{split:"and", join:"and", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "exactly one of `split` and `join`",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"xor", branches:{}}}
               b: {kind:"noop"}"#,
            "must contain at least one branch",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:"true"}}}
               b: {kind:"noop"}"#,
            "must be unconditional",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"xor", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "requires a condition",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"xor", branches:{b:"params.x"}, default:"missing"}}
               b: {kind:"noop"}"#,
            "is not one of the declared branches",
        ),
        (
            r#"a: {kind:"gateway", gateway:{join:"or"}}
               b: {kind:"noop"}"#,
            "must not be empty",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop", needs:["a"]}"#,
            "must not list split gateway",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop", needs:["c"]}
               c: {kind:"noop"}"#,
            "must be empty",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}}}
               b: {kind:"gateway", needs:["c"], gateway:{join:"and"}}
               c: {kind:"noop"}"#,
            "joins cannot be branch targets",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{missing:true}}}
               b: {kind:"noop"}"#,
            "targets unknown step",
        ),
    ];
    for (steps, expected) in cases {
        let source =
            format!("package demo\nworkflow: {{ name: \"bad\"\n steps: {{ {steps} }} }}\n");
        let error = compile_error(&source);
        assert!(
            error.contains(expected),
            "expected `{expected}` in error: {error}"
        );
    }
}

#[test]
fn branch_condition_scope_is_enforced_at_compile_time() {
    let downstream = r#"
package demo
workflow: {
    name: "scope"
    steps: {
        test: { kind: "noop" }
        route: {
            kind: "gateway"
            needs: ["test"]
            gateway: { split: "xor", branches: {
                deployFast: "steps.deployFast.exitCode == 0"
                deploySlow: true
            }, default: "deploySlow" }
        }
        deployFast: { kind: "noop" }
        deploySlow: { kind: "noop" }
    }
}
"#;
    let error = compile_error(downstream);
    assert!(error.contains("not a transitive predecessor"), "{error}");

    let predecessor = r#"
package demo
workflow: {
    name: "scope"
    steps: {
        test: { kind: "noop" }
        route: {
            kind: "gateway"
            needs: ["test"]
            gateway: { split: "xor", branches: {
                deployFast: "stepStatus.test == \"succeeded\""
                deploySlow: true
            }, default: "deploySlow" }
        }
        deployFast: { kind: "noop" }
        deploySlow: { kind: "noop" }
    }
}
"#;
    ostinato_lang::compile(predecessor).expect("predecessor status is in scope");

    let unknown_step = predecessor.replace("stepStatus.test", "stepStatus.nope");
    let error = compile_error(&unknown_step);
    assert!(
        error.contains("unknown reference") || error.contains("not a transitive predecessor"),
        "{error}"
    );

    let unknown_param = predecessor.replace("stepStatus.test", "params.nope");
    let error = compile_error(&unknown_param);
    assert!(error.contains("unknown reference"), "{error}");
}

#[test]
fn gateway_compiles_identically_through_value_and_json() {
    let file = ostinato_lang::parse(GATEWAY_SOURCE).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let from_value = ostinato_lang::compile_value(&value).expect("compile value");
    let json = value.to_json().expect("to json");
    let from_json = ostinato_lang::compile_json(&json).expect("compile json");
    assert_eq!(from_value, from_json);
}

#[test]
fn gateway_additional_validation_rules_are_path_qualified() {
    let cases = [
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}
               c: {kind:"noop", needs:["a"]}"#,
            "workflow.steps.c.needs must not list split gateway `a`",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}
               m: {kind:"gateway", needs:["a"], gateway:{join:"or"}}"#,
            "workflow.steps.m.needs must not list split gateway `a`",
        ),
        (
            r#"a: {kind:"noop"}
               m: {kind:"gateway", needs:["a"], gateway:{join:"or", branches:{}}}"#,
            "a join gateway must not declare `branches` or `default`",
        ),
        (
            r#"a: {kind:"noop"}
               m: {kind:"gateway", needs:["a"], gateway:{join:"or", default:"a"}}"#,
            "a join gateway must not declare `branches` or `default`",
        ),
        (
            r#"a: {kind:"gateway", gateway:{}}"#,
            "one of `split` or `join` is required",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{a:true}}}"#,
            "must not target the gateway itself",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"nand", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "unknown split mode",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:false}}}
               b: {kind:"noop"}"#,
            "`false` branches are not allowed",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:{x:1}}}}
               b: {kind:"noop"}"#,
            "must be `true` or a condition string",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}, extra:1}}
               b: {kind:"noop"}"#,
            "unknown field `extra`",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"and", branches:{b:true}, default:"b"}}
               b: {kind:"noop"}"#,
            "not allowed on an AND split",
        ),
        (
            r#"a: {kind:"gateway", gateway:{split:"xor", branches:{b:"true", c:"true"}, default:"c"}}
               b: {kind:"noop"}
               c: {kind:"noop"}"#,
            "is the default branch and must be `true`",
        ),
        (
            r#"a: {kind:"gateway", retry:{max:1}, gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "retry is not allowed",
        ),
        (
            r#"a: {kind:"gateway", continueOnError:true, gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "continueOnError is not allowed",
        ),
        (
            r#"a: {kind:"gateway", timeoutMs:5, gateway:{split:"and", branches:{b:true}}}
               b: {kind:"noop"}"#,
            "timeoutMs is not allowed",
        ),
        (
            r#"test: {kind:"noop"}
               a: {kind:"gateway", needs:["test"], gateway:{split:"xor", branches:{b:"steps.test.score >=", c:true}, default:"c"}}
               b: {kind:"noop"}
               c: {kind:"noop"}"#,
            "is invalid",
        ),
    ];
    for (steps, expected) in cases {
        let source =
            format!("package demo\nworkflow: {{ name: \"bad\"\n steps: {{ {steps} }} }}\n");
        let error = compile_error(&source);
        assert!(
            error.contains(expected),
            "expected `{expected}` in error: {error}"
        );
    }
}

#[test]
fn gateway_cycles_and_implicit_joins_are_rejected_at_compile_time() {
    let cycle = r#"
package demo
workflow: {
    name: "both"
    steps: {
        a: { kind: "noop", needs: ["b"] }
        b: { kind: "noop", needs: ["a"], cycle: {max: 2} }
        gate: { kind: "gateway", gateway: { split: "and", branches: {c: true} } }
        c: { kind: "noop" }
    }
}
"#;
    ostinato_lang::compile(cycle).expect("a gateway outside a legacy cycle is allowed");

    let implicit = r#"
package demo
workflow: {
    name: "implicit"
    steps: {
        a: { kind: "noop" }
        b: { kind: "noop" }
        c: { kind: "noop", needs: ["a", "b"] }
        gate: { kind: "gateway", gateway: { split: "and", branches: {d: true} } }
        d: { kind: "noop" }
    }
}
"#;
    let error = compile_error(implicit);
    assert!(error.contains("workflow.steps.c"), "{error}");
}

#[test]
fn gateway_condition_scope_accepts_transitive_predecessors_and_ignores_string_literals() {
    let transitive = r#"
package demo
params: { label: string | *"steps.deployFast" }
workflow: {
    name: "scope-ok"
    steps: {
        test: { kind: "noop" }
        mid: { kind: "noop", needs: ["test"] }
        route: {
            kind: "gateway"
            needs: ["mid"]
            gateway: { split: "xor", branches: {
                deployFast: "steps.test.exitCode == 0 && params.label == \"steps.deployFast\""
                deploySlow: true
            }, default: "deploySlow" }
        }
        deployFast: { kind: "noop" }
        deploySlow: { kind: "noop" }
    }
}
"#;
    ostinato_lang::compile(transitive)
        .expect("transitive predecessor and string literal are accepted");
}

#[test]
fn gateway_definition_backed_branches_compile_identically() {
    let source = r#"
package demo
params: { fast: bool | *true }
#Branches: {
    deployFast: "params.fast"
    deploySlow: true
}
workflow: {
    name: "definitions"
    steps: {
        test: { kind: "noop" }
        route: {
            kind: "gateway"
            needs: ["test"]
            gateway: { split: "xor", branches: #Branches, default: "deploySlow" }
        }
        deployFast: { kind: "noop" }
        deploySlow: { kind: "noop" }
    }
}
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval");
    let from_value = ostinato_lang::compile_value(&value).expect("compile value");
    let from_json = ostinato_lang::compile_json(&value.to_json().expect("json")).expect("json");
    assert_eq!(from_value, from_json);
    assert_eq!(
        from_value
            .step("route")
            .and_then(|step| step.gateway.as_ref())
            .and_then(|gateway| gateway.branches.get("deployFast"))
            .and_then(|branch| branch.condition.as_deref()),
        Some("params.fast")
    );
}
#[test]
fn definitions_can_select_their_own_fields() {
    let source = r#"
package demo
#D: {a: #D.b, b: 1}
#N: {a: #N.b.c, b: {c: 2}}
#P: {a: #P.b, b: int | *1}
#Deep: {inner: {x: #Deep.b}, b: 1}
#List: {a: #List.l[0], l: [7, 8]}
#Hidden: {a: #Hidden.#I, #I: 3}
#Quoted: {a: #Quoted["b"], b: 1}
#Arith: {a: #Arith.b + 1, b: 1}
#Composed: {a: #Composed.b} & {b: 1}
base: {b: 1}
#FromBase: base & {a: #FromBase.b}
#Outer: {#Inner: {a: #Outer.b}, b: 1, x: #Inner.a}
#G: {if true {b: 1}, a: #G.b}
#G2: {for k, v in {b: 2} {"\(k)": v}, a: #G2.b}
#G3: {for i, v in [{b: 3}] {"x": v.b}, a: #G3.x}
#G4: {if true {_b: 1}, a: #G4._b}
#GS: {for k, v in {b: 9, y: #GS.b} {"out-\(k)": v}, b: 1}
#M: {b?: int, if true {b: 5}, a: #M.b}
#MF: {b?: int, for k, v in {b: 6} {"\(k)": v}, a: #MF.b}
regular: {a: regular.b, b: 1}
regular_deep: {inner: {a: regular_deep.b}, b: 1}
#Shadow: {#Shadow: {b: 9}, a: #Shadow.b, b: 1}
shadow: {shadow: {b: 9}, a: shadow.b, b: 1}
d: #D
n: #N
p: #P
deep: #Deep
list: #List
hidden: #Hidden
quoted: #Quoted
arith: #Arith
composed: #Composed
from_base: #FromBase
outer: #Outer
shadow_def: #Shadow
g: #G
g2: #G2
g3: #G3
g4: #G4
gs_y: #GS."out-y"
gs_b: #GS."out-b"
m: #M
mf: #MF
"#;
    let file = ostinato_lang::parse(source).expect("parse self-selection");
    let value = ostinato_lang::eval_file(&file).expect("eval self-selection");
    let json = value.to_json().expect("json");
    assert_eq!(json["d"]["a"], 1);
    assert_eq!(json["n"]["a"], 2);
    assert_eq!(json["p"]["a"], 1);
    assert_eq!(json["deep"]["inner"]["x"], 1);
    assert_eq!(json["list"]["a"], 7);
    assert_eq!(json["hidden"]["a"], 3);
    assert_eq!(json["quoted"]["a"], 1);
    assert_eq!(json["arith"]["a"], 2);
    assert_eq!(json["composed"]["a"], 1);
    assert_eq!(json["from_base"]["a"], 1);
    assert_eq!(json["outer"]["x"], 1);
    assert_eq!(json["regular"]["a"], 1);
    assert_eq!(json["regular_deep"]["inner"]["a"], 1);
    assert_eq!(json["shadow_def"]["a"], 9);
    assert_eq!(json["shadow"]["a"], 9);
    assert_eq!(json["g"]["a"], 1);
    assert_eq!(json["g2"]["a"], 2);
    assert_eq!(json["g3"]["a"], 3);
    assert_eq!(json["g4"]["a"], 1);
    assert_eq!(json["gs_y"], 1);
    assert_eq!(json["gs_b"], 9);
    assert_eq!(json["m"]["a"], 5);
    assert_eq!(json["mf"]["a"], 6);

    for source in [
        "#D: {a: #D.b, b?: int}",
        "#D: {a: #D.b, b?: int | *1}",
        "#D: {a: #D.b, b: #D.a}",
        "#D: {if true {b?: int}, a: #D.b}",
    ] {
        let file =
            ostinato_lang::parse(&format!("package demo\n{source}\nx: #D.a\n")).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional or cyclic self-selection must fail: {source}"
        );
    }
}

#[test]
fn self_selection_resolves_same_body_comprehension_fields() {
    let source = "package demo\n#D: {if true {b: 1, y: #D.b}, a: #D.y}\nx: #D.a\n";
    let file = ostinato_lang::parse(source).expect("parse");
    let value = ostinato_lang::eval_file(&file).expect("eval same-body comprehension");
    assert_eq!(value.to_json().expect("json")["x"], 1);

    let generated = "package demo\n#M: {b?: int, if true {b: 5, y: #M.b}}\nx: #M\n";
    let file = ostinato_lang::parse(generated).expect("parse");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval generated body field")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["b"], 5);
    assert_eq!(json["x"]["y"], 5);
}

#[test]
fn required_fields_can_depend_on_materialized_optional_fields() {
    let source = r#"
package demo
#P: {name?: string, greeting: "Hello \(name)"}
#A: {n?: int, total: n + 1}
#C: {a?: int, b: a + 1, c: b + 1}
greeting: #P & {name: "Alice"}
arithmetic: #A & {n: 4}
chain: #C & {a: 1}
defaulted: #A & {n: *4 | int}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional-dependent fields");
    let value = ostinato_lang::eval_file(&file).expect("eval optional-dependent fields");
    let json = value.to_json().expect("json");
    assert_eq!(json["greeting"]["greeting"], "Hello Alice");
    assert_eq!(json["arithmetic"]["total"], 5);
    assert_eq!(json["chain"]["b"], 2);
    assert_eq!(json["chain"]["c"], 3);
    assert_eq!(json["defaulted"]["total"], 5);

    let unmaterialized = "package demo\n#P: {name?: string, greeting: \"Hello \\(name)\"}\nx: #P\n";
    let file = ostinato_lang::parse(unmaterialized).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "unmaterialized optional references must still fail"
    );

    let compound = r#"
package demo
#E: {a?: int, v: a}
#P: {ls?: [[{a?: int, v: a}]], e?: #E}
list_case: #P & {ls: [[{a: 2}]]}
def_case: #P & {e: {a: 2}}
#N: {cfg: {a?: int, v: a}}
nested_case: #N & {cfg: {a: 2}}
#L: {xs?: [int, int], sum: xs[0] + xs[1]}
list_sum: #L & {xs: [4, 5]}
#T: {cfg?: {port: int, url: "http://localhost:\(port)"}}
dependent_struct: #T & {cfg: {port: 8080}}
"#;
    let file = ostinato_lang::parse(compound).expect("parse compound optional-dependent fields");
    let value = ostinato_lang::eval_file(&file).expect("eval compound optional-dependent fields");
    let json = value.to_json().expect("json");
    assert_eq!(
        json["list_case"]["ls"],
        serde_json::json!([[{"a": 2, "v": 2}]])
    );
    assert_eq!(json["def_case"]["e"], serde_json::json!({"a": 2, "v": 2}));
    assert_eq!(
        json["nested_case"]["cfg"],
        serde_json::json!({"a": 2, "v": 2})
    );
    assert_eq!(json["list_sum"]["xs"], serde_json::json!([4, 5]));
    assert_eq!(json["list_sum"]["sum"], 9);
    assert_eq!(
        json["dependent_struct"]["cfg"],
        serde_json::json!({"port": 8080, "url": "http://localhost:8080"})
    );

    let params = r#"
package demo
#P: {name?: string, greeting: "Hello \(name)"}
params: #P & {name: "Alice"}
workflow: {name: "\(params.greeting)", steps: {s: {kind: "noop"}}}
"#;
    let file = ostinato_lang::parse(params).expect("parse definition-backed params");
    let value = ostinato_lang::eval_file(&file).expect("eval definition-backed params");
    let json = value.to_json().expect("json");
    assert_eq!(json["params"]["name"], "Alice");
    assert_eq!(json["params"]["greeting"], "Hello Alice");
    assert_eq!(json["workflow"]["name"], "Hello Alice");
}

#[test]
fn reusable_non_definition_bases_resolve_dependent_optionals() {
    let source = r#"
package demo
let base = {b?: int, c: b}
_base: {b?: int, c: b}
#A: base & {b: 3}
#B: _base & {b: 4}
let_case: #A
hidden_case: #B
concrete: #A & {c: 3}
"#;
    let file = ostinato_lang::parse(source).expect("parse reusable bases");
    let value = ostinato_lang::eval_file(&file).expect("eval reusable bases");
    let json = value.to_json().expect("json");
    assert_eq!(json["let_case"], serde_json::json!({"b": 3, "c": 3}));
    assert_eq!(json["hidden_case"], serde_json::json!({"b": 4, "c": 4}));
    assert_eq!(json["concrete"], serde_json::json!({"b": 3, "c": 3}));
}

#[test]
fn compound_optional_edges_match_cue() {
    let source = r#"
package demo
#P: {a?: int, b: a + 1}
let base = #P
let struct_base = {cfg?: {a?: int, v: a}}
#A: struct_base & {cfg: {a: 3}}
let_definition: base & {a: 2}
let_struct: #A
#Absent: {cfg?: {a: int, b: a + 1}}
absent: #Absent & {}
#PC: {cfg?: {a: int, b: a + 1}, v: cfg.b}
compound: #PC & {cfg: {a: 2}}
#N: {cfg: {a?: int, v: a}}
nested_required: #N & {cfg: {a: 2}}
#E: {a?: int, v: a}
#PD: {e?: #E, w: e.v}
definition: #PD & {e: {a: 2}}
#PL: {ls?: [[{a?: int, v: a}]]}
list: #PL & {ls: [[{a: 2}]]}
"#;
    let file = ostinato_lang::parse(source).expect("parse compound edges");
    let value = ostinato_lang::eval_file(&file).expect("eval compound edges");
    let json = value.to_json().expect("json");
    assert_eq!(json["let_definition"], serde_json::json!({"a": 2, "b": 3}));
    assert_eq!(
        json["let_struct"],
        serde_json::json!({"cfg": {"a": 3, "v": 3}})
    );
    assert_eq!(json["absent"], serde_json::json!({}));
    assert_eq!(
        json["compound"],
        serde_json::json!({"cfg": {"a": 2, "b": 3}, "v": 3})
    );
    assert_eq!(
        json["nested_required"],
        serde_json::json!({"cfg": {"a": 2, "v": 2}})
    );
    assert_eq!(
        json["definition"],
        serde_json::json!({"e": {"a": 2, "v": 2}, "w": 2})
    );
    assert_eq!(
        json["list"],
        serde_json::json!({"ls": [[{"a": 2, "v": 2}]]})
    );

    for source in [
        "package demo\n#P: {a: int, v: #P.a + 1}\nx: #P & {a: 2}\n",
        "package demo\n#P: {cfg?: {a?: int, v: #P.cfg.a + 1}}\nx: #P & {cfg: {a: 2}}\n",
        "package demo\n#P: {cfg?: {a: int, b: a + 1}, v: cfg.b}\nx: #P & {}\n",
        "package demo\n#P: {cfg?: {a: int}}\nx: #P & {v: cfg}\n",
        "package demo\n#P: {cfg?: {a: int}}\nx: #P & {v: #P.cfg.a}\n",
        "package demo\n#P: {cfg?: {a: int}}\nx: #P & {cfg: {a: \"s\"}}\n",
        "package demo\n#P: {xs?: [int, int]}\nx: #P & {xs: [1, 2, 3]}\n",
        "package demo\n#P: {xs?: [int, int]}\nx: #P & {xs: [1, \"s\"]}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse rejected compound edge");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "compound optional edge must reject: {source:?}"
        );
    }

    let eval_params = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse params edge");
        let value = ostinato_lang::eval_file(&file).expect("eval params edge");
        value.to_json().expect("json")["params"].clone()
    };
    assert_eq!(
        eval_params("package demo\n#P: {n: *2 | int, m: n * 2}\nparams: #P & {n: 5}\n"),
        serde_json::json!({"n": 5, "m": 10})
    );
    assert_eq!(
        eval_params("package demo\n#P: {n: *2 | int, m: n * 2}\nparams: #P\n"),
        serde_json::json!({"n": 2, "m": 4})
    );
    assert_eq!(
        eval_params(
            "package demo\n#P: {cfg?: {a: int, b: a + 1}, v: cfg.b}\nparams: #P & {cfg: {a: 2}}\n"
        ),
        serde_json::json!({"cfg": {"a": 2, "b": 3}, "v": 3})
    );
    assert_eq!(
        eval_params("package demo\n#N: {cfg: {a?: int, v: a}}\nparams: #N & {cfg: {a: 2}}\n"),
        serde_json::json!({"cfg": {"a": 2, "v": 2}})
    );
    assert_eq!(
        eval_params(
            "package demo\n#E: {a?: int, v: a}\n#P: {e?: #E, w: e.v}\nparams: #P & {e: {a: 2}}\n"
        ),
        serde_json::json!({"e": {"a": 2, "v": 2}, "w": 2})
    );
    assert_eq!(
        eval_params(
            "package demo\n#P: {ls?: [[{a?: int, v: a}]]}\nparams: #P & {ls: [[{a: 2}]]}\n"
        ),
        serde_json::json!({"ls": [[{"a": 2, "v": 2}]]})
    );
}

#[test]
fn nested_defaults_and_let_injection_match_cue() {
    let eval_params = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse nested default params");
        let value = ostinato_lang::eval_file(&file).expect("eval nested default params");
        value.to_json().expect("json")["params"].clone()
    };
    assert_eq!(
        eval_params(
            "package demo\n#P: {cfg?: {n: *2 | int, m: n * 3}, v: cfg.m}\nparams: #P & {cfg: {n: 5}}\n"
        ),
        serde_json::json!({"cfg": {"n": 5, "m": 15}, "v": 15})
    );
    assert_eq!(
        eval_params(
            "package demo\n#P: {cfg: {n: *2 | int, m: n * 3}, v: cfg.m}\nparams: #P & {cfg: {n: 5}}\n"
        ),
        serde_json::json!({"cfg": {"n": 5, "m": 15}, "v": 15})
    );
    assert_eq!(
        eval_params(
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {e?: #E, v: e.m}\nparams: #P & {e: {n: 5}}\n"
        ),
        serde_json::json!({"e": {"n": 5, "m": 15}, "v": 15})
    );
    assert_eq!(
        eval_params(
            "package demo\n#P: {ls?: [{n: *2 | int, m: n * 3}], v: ls[0].m}\nparams: #P & {ls: [{n: 5}]}\n"
        ),
        serde_json::json!({"ls": [{"n": 5, "m": 15}], "v": 15})
    );

    let source = r#"
package demo
#P: {a?: int, b: a + 1}
let base = #P & {}
let plain = (#P)
x: base & {a: 2}
y: plain & {a: 4}
"#;
    let file = ostinato_lang::parse(source).expect("parse injected let bases");
    let value = ostinato_lang::eval_file(&file).expect("eval injected let bases");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 2, "b": 3}));
    assert_eq!(json["y"], serde_json::json!({"a": 4, "b": 5}));
    assert_eq!(
        eval_params(
            "package demo\n#P: {a?: int, b: a + 1}\nlet base = #P\nparams: base & {a: 2}\n"
        ),
        serde_json::json!({"a": 2, "b": 3})
    );

    let hidden = "package demo\n_h: 1 + int\nx: 1\n";
    let file = ostinato_lang::parse(hidden).expect("parse hidden operation error");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "hidden operation errors must not be skipped"
    );
    for source in [
        "package demo\n_h: int\nx: 1\n",
        "package demo\n_h: {a?: int, c: a}\nx: 1\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse incomplete hidden");
        assert!(
            ostinato_lang::eval_file(&file).is_ok(),
            "incomplete hidden fields are allowed: {source:?}"
        );
    }
}

#[test]
fn nested_definitions_inside_params_literals_match_cue() {
    let eval_params = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse nested definition params");
        let value = ostinato_lang::eval_file(&file).expect("eval nested definition params");
        value.to_json().expect("json")["params"].clone()
    };
    let cases = [
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {ls?: [#E], v: ls[0].m}\nparams: #P & {ls: [{n: 4}]}\n",
            serde_json::json!({"ls": [{"n": 4, "m": 12}], "v": 12}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {c?: {e: #E}, v: c.e.m}\nparams: #P & {c: {e: {n: 4}}}\n",
            serde_json::json!({"c": {"e": {"n": 4, "m": 12}}, "v": 12}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {ls?: [#E, #E], v: ls[0].m + ls[1].m}\nparams: #P & {ls: [{n: 4}, {n: 5}]}\n",
            serde_json::json!({"ls": [{"n": 4, "m": 12}, {"n": 5, "m": 15}], "v": 27}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {ls?: [[#E]], v: ls[0][0].m}\nparams: #P & {ls: [[{n: 4}]]}\n",
            serde_json::json!({"ls": [[{"n": 4, "m": 12}]], "v": 12}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {ls?: [int, #E], v: ls[1].m}\nparams: #P & {ls: [7, {n: 4}]}\n",
            serde_json::json!({"ls": [7, {"n": 4, "m": 12}], "v": 12}),
        ),
        (
            "package demo\nlet base = {n: *2 | int, m: n * 3}\n#P: {ls?: [base], v: ls[0].m}\nparams: #P & {ls: [{n: 4}]}\n",
            serde_json::json!({"ls": [{"n": 4, "m": 12}], "v": 12}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {cfg?: {inner: {e: #E}}, v: cfg.inner.e.m}\nparams: #P & {cfg: {inner: {e: {n: 4}}}}\n",
            serde_json::json!({"cfg": {"inner": {"e": {"n": 4, "m": 12}}}, "v": 12}),
        ),
        (
            "package demo\n#E: {n: *2 | int, m: n * 3}\nlet cfg = {n: 4}\n#P: {ls?: [#E], v: ls[0].m}\nparams: #P & {ls: [cfg]}\n",
            serde_json::json!({"ls": [{"n": 4, "m": 12}], "v": 12}),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(eval_params(source), expected, "source: {source}");
    }

    let closed = "package demo\n#E: {n: *2 | int, m: n * 3}\nlet cfg = {n: 4}\n#P: {ls?: [cfg], v: ls[0].m}\nparams: #P & {ls: [#E]}\n";
    let file = ostinato_lang::parse(closed).expect("parse closed nested definition params");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "closed nested definition must reject extra defaulted fields"
    );
}

#[test]
fn optional_injection_soundness_and_incomplete_export_match_cue() {
    for source in [
        "package demo\n#P: {a?: {x: int}, b: a}\nx: #P & {a: {x: 1}, b: {x: 2}}\n",
        "package demo\n#P: {a?: [int], b: [a]}\nx: #P & {a: [1], b: [[2]]}\n",
        "package demo\n#P: {a?: {x: int}, b: {inner: a}}\nx: #P & {a: {x: 1}, b: {inner: {x: 2}}}\n",
        "package demo\n#E: {m: {x: int}, n: m}\n#P: {e?: #E}\nx: #P & {e: {m: {x: 1}, n: {x: 2}}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse sibling injection conflict");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "sibling injection must conflict: {source:?}"
        );
    }

    let non_params = r#"
package demo
#E: {n: *2 | int, m: n * 3}
#P: {ls?: [#E]}
x: #P & {ls: [{n: 4}]}
"#;
    let file = ostinato_lang::parse(non_params).expect("parse non-params narrowing");
    let value = ostinato_lang::eval_file(&file).expect("eval non-params narrowing");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["ls"], serde_json::json!([{"n": 4, "m": 12}]));

    let conflicting = "package demo\n#E: {m: {x: int}, n: m}\n#P: {e?: #E}\nx: #P & {e: {m: {x: 1}, n: {x: 2}}}\n";
    let file = ostinato_lang::parse(conflicting).expect("parse non-params definition conflict");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "definition sibling injection must conflict"
    );

    let export_mode = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse export source");
        ostinato_lang::eval_file_with_imports_mode(&file, &BTreeMap::new(), &BTreeMap::new(), true)
    };
    for source in [
        "package demo\n#D: {a: int, b: a + 1}\nx: 1\n",
        "package demo\n_h: {a: int, b: a + 1}\nx: 1\n",
    ] {
        let value = export_mode(source).expect("soft incomplete fields must export");
        assert_eq!(value.to_json().expect("json")["x"], 1);
    }
    for source in [
        "package demo\n_h: 1 + int\nx: 1\n",
        "package demo\n#D: {a: int, b: 1 + int}\nx: 1\n",
        "package demo\n#P: {a?: int, b: a, c: 1, c: 2}\nx: 1\n",
        "package demo\n_h: {a?: int, b: a, z: [1][2]}\nx: 1\n",
    ] {
        let exported = export_mode(source);
        assert!(
            exported.is_err(),
            "hard errors under incomplete fields must not be skipped: {source:?}"
        );
    }
}

#[test]
fn split_unification_merges_overrides_before_evaluation() {
    let eval = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse split unification");
        let value = ostinato_lang::eval_file(&file).expect("eval split unification");
        value.to_json().expect("json")
    };
    let params = eval(
        "package demo\n#P: {a?: int, b?: int, sum: a + b}\nparams: {p: #P & {a: 1} & {b: 2}}\n",
    );
    assert_eq!(
        params["params"]["p"],
        serde_json::json!({"a": 1, "b": 2, "sum": 3})
    );
    let defaults =
        eval("package demo\n#P: {a: *1 | int, b: *2 | int, sum: a + b}\nx: #P & {a: 3} & {b: 4}\n");
    assert_eq!(defaults["x"], serde_json::json!({"a": 3, "b": 4, "sum": 7}));
    let nested = eval(
        "package demo\n#P: {cfg?: {n: *2 | int, m: n * 3}, v: cfg.m}\nx: #P & {} & {cfg: {n: 5}}\n",
    );
    assert_eq!(
        nested["x"],
        serde_json::json!({"cfg": {"n": 5, "m": 15}, "v": 15})
    );
    let inline = eval("package demo\nx: {a?: int, b?: int, sum: a + b} & {a: 1} & {b: 2}\n");
    assert_eq!(inline["x"], serde_json::json!({"a": 1, "b": 2, "sum": 3}));
    let required =
        eval("package demo\n#P: {a: int, b: int, sum: a + b}\nx: #P & {a: 1} & {b: 2}\n");
    assert_eq!(required["x"], serde_json::json!({"a": 1, "b": 2, "sum": 3}));
    let optional_struct = eval(
        "package demo\n#P: {cfg?: {n: int, m: int}, v: cfg.m}\nx: #P & {cfg: {n: 1}} & {cfg: {m: 2}}\n",
    );
    assert_eq!(
        optional_struct["x"],
        serde_json::json!({"cfg": {"n": 1, "m": 2}, "v": 2})
    );
    let list = eval(
        "package demo\n#P: {ls?: [{n: int, m: int}], v: ls[0].m}\nx: #P & {ls: [{n: 1}]} & {ls: [{m: 2}]}\n",
    );
    assert_eq!(
        list["x"],
        serde_json::json!({"ls": [{"n": 1, "m": 2}], "v": 2})
    );
    let let_base = eval(
        "package demo\n#P: {a?: int, b?: int, sum: a + b}\nlet base = #P & {a: 1}\nx: base & {b: 2}\n",
    );
    assert_eq!(let_base["x"], serde_json::json!({"a": 1, "b": 2, "sum": 3}));
    let alias = eval("package demo\n#E: {n: *2 | int, m: n * 3}\n#A: #E\nx: #A & {n: 6}\n");
    assert_eq!(alias["x"], serde_json::json!({"n": 6, "m": 18}));
}

#[test]
fn optional_definition_aliases_are_not_materialized() {
    for source in [
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\nx: #A & {n: 6}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\nx?: #E\ny: x & {n: 6}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\n#B: #A\nx: #B & {n: 6}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\n#B: #A\n#C: #B\nx: #C & {n: 6}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\nx: (#A) & {n: 6}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\nx: #A & {n: 6} & {}\n",
        "package demo\n#E: {n: *2 | int, m: n * 3}\n#A?: #E\nlet L = #A\ny: L & {n: 6}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse optional alias");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional definition aliases must not materialize: {source:?}"
        );
    }

    let eval = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse required alias");
        let value = ostinato_lang::eval_file(&file).expect("eval required alias");
        value.to_json().expect("json")
    };
    let chain =
        eval("package demo\n#E: {n: *2 | int, m: n * 3}\n#A: #E\n#B: #A\n#C: #B\nx: #C & {n: 6}\n");
    assert_eq!(chain["x"], serde_json::json!({"n": 6, "m": 18}));
    let split = eval("package demo\n#E: {n: *2 | int, m: n * 3}\n#A: #E\nx: #A & {n: 6} & {}\n");
    assert_eq!(split["x"], serde_json::json!({"n": 6, "m": 18}));
}

#[test]
fn optional_disjunction_alternatives_are_unavailable() {
    let source = r#"
package demo
#E: {n: int, m: n + 1}
#P: {cfg?: {n: int}, ls?: [int]}
#A?: #E
#B: #A
x?: #E
let L = #A
def:     #A | {n: 1}
field:   x | {n: 2}
multi:   #A | #A | {n: 3}
alias:   #B | {n: 4}
structs: #P.cfg | {n: 5}
lists:   #P.ls | [6]
default: #A | *{n: 7}
both:    #A | {n: "s"}
via_let: L | {n: 8}
params: {p: #A | {n: 9}}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional disjunction");
    let value = ostinato_lang::eval_file(&file).expect("eval optional disjunction");
    let json = value.to_json().expect("json");
    assert_eq!(json["def"], serde_json::json!({"n": 1}));
    assert_eq!(json["field"], serde_json::json!({"n": 2}));
    assert_eq!(json["multi"], serde_json::json!({"n": 3}));
    assert_eq!(json["alias"], serde_json::json!({"n": 4}));
    assert_eq!(json["structs"], serde_json::json!({"n": 5}));
    assert_eq!(json["lists"], serde_json::json!([6]));
    assert_eq!(json["default"], serde_json::json!({"n": 7}));
    assert_eq!(json["both"], serde_json::json!({"n": "s"}));
    assert_eq!(json["via_let"], serde_json::json!({"n": 8}));
    assert_eq!(json["params"]["p"], serde_json::json!({"n": 9}));

    let soft = "package demo\n_h: int\nx: {b: _h + 1} | {c: 1}\n";
    let file = ostinato_lang::parse(soft).expect("parse soft incomplete disjunction");
    let value = ostinato_lang::eval_file(&file).expect("eval soft incomplete disjunction");
    assert_eq!(
        value.to_json().expect("json")["x"],
        serde_json::json!({"c": 1})
    );

    for source in [
        "package demo\n#E: {n: int, m: n + 1}\n#A?: #E\nx: #A\n",
        "package demo\n#E: {n: int, m: n + 1}\n#A?: #E\n#B?: #E\nx: #A | #B\n",
        "package demo\n#E: {n: int, m: n + 1}\n#A?: #E\ny: (#A | {n: 6}) & {n: 7}\n",
        "package demo\n#A?: {n: int}\nx: {a: #A, b: nope} | {c: 1}\n",
        "package demo\n#A?: int\nx: (#A + int) | 1\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse invalid optional disjunction");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "disjunction without available alternatives must fail: {source:?}"
        );
    }
}

#[test]
fn arbitrary_precision_integer_literals_keep_exact_values() {
    let source = r#"
package demo
big: 123456789012345678901234567890
neg: -123456789012345678901234567890
add: big + 1
sub: big - 1
mul: big * 2
div: big / 2
small_add: 1 + big
gt: big > 1
lt: 1 < big
eq: big == big
ne: big == 1
typed: int & big
bounded: >=1 & big
text: "\(big)"
list: [big, neg]
over: 9223372036854775808
"#;
    let file = ostinato_lang::parse(source).expect("parse big integers");
    let value = ostinato_lang::eval_file(&file).expect("eval big integers");
    let json = value.to_json().expect("json");
    assert_eq!(json["big"].to_string(), "123456789012345678901234567890");
    assert_eq!(json["neg"].to_string(), "-123456789012345678901234567890");
    assert_eq!(json["add"].to_string(), "123456789012345678901234567891");
    assert_eq!(json["sub"].to_string(), "123456789012345678901234567889");
    assert_eq!(json["mul"].to_string(), "246913578024691357802469135780");
    assert_eq!(json["div"].to_string(), "61728394506172839450617283945.0");
    assert_eq!(
        json["small_add"].to_string(),
        "123456789012345678901234567891"
    );
    assert_eq!(json["gt"], true);
    assert_eq!(json["lt"], true);
    assert_eq!(json["eq"], true);
    assert_eq!(json["ne"], false);
    assert_eq!(json["typed"].to_string(), "123456789012345678901234567890");
    assert_eq!(
        json["bounded"].to_string(),
        "123456789012345678901234567890"
    );
    assert_eq!(json["text"], "123456789012345678901234567890");
    assert_eq!(
        json["list"][0].to_string(),
        "123456789012345678901234567890"
    );
    assert_eq!(json["over"].to_string(), "9223372036854775808");

    let out_of_range = "package demo\nx: [1][123456789012345678901234567890]\n";
    let file = ostinato_lang::parse(out_of_range).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "arbitrary-precision list indexes must be range-checked"
    );
}

#[test]
fn big_integer_bounds_and_builtins_match_cue() {
    let source = r#"
package demo
import "math"
import "regexp"
import "time"
import "list"
bounded: (>=9223372036854775808 & <=9223372036854775810) & 9223372036854775809
not_equal: (int & !=123456789012345678901234567890) & 5
sqrt: math.Sqrt(9223372036854775808)
pow: math.Pow(9223372036854775808, 1)
found: regexp.FindAll("a", "aaa", 9223372036854775808)
duration: time.FormatDuration(9223372036854775808)
range: (list.Range(9223372036854775808, 9223372036854775811, 1)[0]) & int
"#;
    let file = ostinato_lang::parse(source).expect("parse big builtins");
    let mut imports = BTreeMap::new();
    for name in ["math", "regexp", "time", "list"] {
        imports.insert(
            name.to_owned(),
            ostinato_lang::builtin_module(name).expect("builtin module"),
        );
    }
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    assert_eq!(json["bounded"].to_string(), "9223372036854775809");
    assert_eq!(json["not_equal"].to_string(), "5");
    assert_eq!(json["sqrt"].to_string(), "3037000499.97605");
    assert_eq!(json["pow"].to_string(), "9223372036854775808");
    assert_eq!(json["found"], serde_json::json!(["a", "a", "a"]));
    assert_eq!(json["duration"], "2562047h47m16.854775807s");
    assert_eq!(json["range"].to_string(), "9223372036854775808");

    let conflict = "package demo\nx: 9223372036854775808 & 9223372036854775808.0\n";
    let file = ostinato_lang::parse(conflict).expect("parse");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "BigInt and float must not unify"
    );

    for (source, module) in [
        (
            "package demo\nimport \"math\"\nx: math.Pow(2, 100000000)\n",
            "math",
        ),
        (
            "package demo\nimport \"time\"\nx: time.FormatDuration(18446744073709551616)\n",
            "time",
        ),
        (
            "package demo\nimport \"time\"\nx: time.FormatDuration(-18446744073709551616)\n",
            "time",
        ),
        (
            "package demo\nimport \"regexp\"\nx: regexp.FindAll(\"a\", \"aaa\", 18446744073709551616)\n",
            "regexp",
        ),
        (
            "package demo\nimport \"regexp\"\nx: regexp.FindAll(\"a\", \"aaa\", -18446744073709551616)\n",
            "regexp",
        ),
    ] {
        let file = ostinato_lang::parse(source).expect("parse");
        let mut local = BTreeMap::new();
        local.insert(
            module.to_owned(),
            ostinato_lang::builtin_module(module).expect("builtin module"),
        );
        assert!(
            ostinato_lang::eval_file_with_imports(&file, &local, &BTreeMap::new()).is_err(),
            "out-of-range big builtin argument must fail: {source:?}"
        );
    }
}

#[test]
fn comprehension_generated_definitions_are_selectable_and_hidden() {
    let source = r#"
package demo
#D: {if true {#I: 3}, a: #D.#I, x: 1}
#Nested: {if true {inner: {#I: 5}}, a: #Nested.inner.#I}
#Both: {if true {#I: 6}, #J: 7, a: #Both.#I, b: #Both.#J}
d: #D
nested: #Nested
both: #Both
"#;
    let file = ostinato_lang::parse(source).expect("parse generated definitions");
    let value = ostinato_lang::eval_file(&file).expect("eval generated definitions");
    let json = value.to_json().expect("json");
    assert_eq!(json["d"]["a"], 3);
    assert_eq!(json["d"]["x"], 1);
    assert!(json["d"].get("#I").is_none());
    assert_eq!(json["nested"]["a"], 5);
    assert_eq!(json["both"]["a"], 6);
    assert_eq!(json["both"]["b"], 7);
    assert!(json["both"].get("#I").is_none());
    assert!(json["both"].get("#J").is_none());

    for source in [
        "#D: {for k, v in {a: 1, b: 2} {#I: v}, x: #D.#I}",
        "#D: {#I: 1, if true {#I: 2}, x: #D.#I}",
    ] {
        let file = ostinato_lang::parse(&format!("package demo\n{source}\nx: #D.x\n"))
            .expect("parse duplicate definitions");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "duplicate generated definitions must conflict: {source}"
        );
    }
}

#[test]
fn comprehension_generated_definitions_respect_caches_and_shadowing() {
    let source = r#"
package demo
#I: 1
#D: {if true {#I: 2}, a: #I}
#M: {#I: int, if true {#I: 3}, a: #I}
#J: {v: int}
if true {#J: {v: 1, w: 2}}
merged: #J
d: #D
m: #M
outer: #I
params: {if true {#I: 3}, x: 1}
"#;
    let file = ostinato_lang::parse(source).expect("parse generated definitions");
    let value = ostinato_lang::eval_file(&file).expect("eval generated definitions");
    let json = value.to_json().expect("json");
    assert_eq!(json["d"]["a"], 1, "outer definition must shadow a bare #I");
    assert_eq!(json["outer"], 1);
    assert_eq!(
        json["m"]["a"], 3,
        "declared and generated definitions must merge"
    );
    assert_eq!(json["params"]["x"], 1);
    assert!(
        json["params"].get("#I").is_none(),
        "generated definitions must not leak into params exports"
    );

    for source in [
        "package demo\n#E: {#I: #Nope, if true {#I: 3}, a: #I}\nx: #E.a\n",
        "package demo\nif true {#I: 3}\nx: #I\n",
        "package demo\nx: {if true {#I: 3}, a: #I}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse invalid generated definitions");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "generated-definition errors/shadowing must fail: {source:?}"
        );
    }
}

#[test]
fn comprehension_generated_declarations_merge_like_cue() {
    let source = r#"
package demo
#I: {v: int, ...}
if true {#I: {...}}
open_merged: #I & {v: 1, z: 3}
#J: {v: int}
if true {#J: {...}}
right_open: #J & {v: 1, w: 2}
#K: {v: int, #M: 4}
if true {#K: {v: 1, #N: 5, w: 2}}
meta_m: #K.#M
meta_n: #K.#N
#D: {if true {#I: {v: int}}, if true {#I: {v: 1, w: 2}}, a: #D.#I}
nested: #D.a
"#;
    let file = ostinato_lang::parse(source).expect("parse declaration merges");
    let value = ostinato_lang::eval_file(&file).expect("eval declaration merges");
    let json = value.to_json().expect("json");
    assert_eq!(json["open_merged"]["z"], 3);
    assert_eq!(json["right_open"]["w"], 2);
    assert_eq!(json["meta_m"], 4);
    assert_eq!(json["meta_n"], 5);
    assert_eq!(json["nested"]["w"], 2);

    let dependent = r#"
package demo
#P: {env: string, greeting: "hello \(env)"}
if true {#P: {env: "prod"}}
x: #P
"#;
    let file = ostinato_lang::parse(dependent).expect("parse dependent merge");
    let value = ostinato_lang::eval_file(&file).expect("eval dependent merge");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["env"], "prod");
    assert_eq!(json["x"]["greeting"], "hello prod");
}

#[test]
fn optional_open_metadata_on_struct_literals_matches_cue() {
    let source = r#"
package demo
base: {b?: int}
#D: base
nested_base: {inner: {b?: int, c: 1}}
open_base: {c: 1, ...}
opt_pattern: {[string]: int} & {b?: int, c: 1}
def_empty: #D
def_set: #D & {b: 1}
optional_export: {b?: int, c: 1}
nested: nested_base & {inner: {b: 2}}
open_lit: open_base & {d: 2}
open_nested: {inner: {c: 1, ...}} & {inner: {d: 2}}
opt_list: [{b?: int, c: 1}] & [{b: 2}]
open_list: [...{b?: int, c: 1}] & [{b: 2}]
pattern: opt_pattern & {b: 2}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional/open metadata");
    let value = ostinato_lang::eval_file(&file).expect("eval optional/open metadata");
    let json = value.to_json().expect("json");
    assert_eq!(json["def_empty"], serde_json::json!({}));
    assert_eq!(json["def_set"]["b"], 1);
    assert_eq!(json["optional_export"], serde_json::json!({"c": 1}));
    assert_eq!(json["nested"]["inner"]["b"], 2);
    assert_eq!(json["nested"]["inner"]["c"], 1);
    assert_eq!(json["open_lit"]["d"], 2);
    assert_eq!(json["open_nested"]["inner"]["d"], 2);
    assert_eq!(json["opt_list"][0]["b"], 2);
    assert_eq!(json["open_list"][0]["c"], 1);
    assert_eq!(json["pattern"]["b"], 2);

    for source in [
        "package demo\nx: {b?: string} & {b: 1}\n",
        "package demo\nbase: {b?: int, c: b}\nx: base & {b: 2}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse invalid optional/open metadata");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "invalid optional/open literal must fail: {source:?}"
        );
    }
}

#[test]
fn optional_fields_do_not_leak_through_unification_and_vars_shadow() {
    for source in [
        "package demo\nx?: {a: int}\ny: x & {a: 1}\n",
        "package demo\n_x?: {a: int}\ny: _x & {a: 1}\n",
        "package demo\n#D: {x?: {a: int}, y: x & {a: 1}}\nz: #D\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse optional unification");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional fields must not be referenceable through &: {source:?}"
        );
    }

    let source = r#"
package demo
base: {a: 1}
list: [for base in [{a: 2}] {base & {b: 3}}]
struct_comp: {for base in [{a: 2}] {v: base & {b: 3}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse comprehension shadowing");
    let value = ostinato_lang::eval_file(&file).expect("eval comprehension shadowing");
    let json = value.to_json().expect("json");
    assert_eq!(json["list"][0]["a"], 2);
    assert_eq!(json["list"][0]["b"], 3);
    assert_eq!(json["struct_comp"]["v"]["a"], 2);
    assert_eq!(json["struct_comp"]["v"]["b"], 3);
}

#[test]
fn narrowed_defaults_recompute_dependent_fields() {
    let source = r#"
package demo
#A: {n: *4 | 6, total: n + 1}
#B: {n: int | *1, y: n + 1}
#C: {n: *1 | int, l: [n, n + 1]}
#D: {a: *1 | int, b: *2 | int, sum: a + b}
#E: {s: *"a" | string, y: "x\(s)"}
#F: {n?: int | *1, y: n + 1}
simple: #A & {n: 6}
disjunction: #A & {n: >4}
typed: #B & {n: 3}
list: #C & {n: 3}
two: #D & {a: 5, b: 7}
optional: #F & {n: 4}
string_value: #E & {s: "b"}
direct: {x: int | *1, y: x + 1} & {x: 3}
#G: {a: {b: int | *1, c: b + 1}, d: a.c + 1}
nested: #G & {a: {b: 5}}
#L: {l: [{b: int | *1, c: b + 1}], d: l[0].c + 1}
list_nested: #L & {l: [{b: 5}]}
#AB: 1
#S: {n: *1 | int, ref: #AB, label: "x#S", total: n + 1}
substring: #S & {n: 3}
#Pair: {l: [{b: int | *1, c: b + 1}, {b: int | *1, c: b + 1}]}
pair: #Pair & {l: [{b: 5}, {b: 6}]}
#LL: {l: [[{b: int | *1, c: b + 1}]], d: l[0][0].c + 1}
nested_list: #LL & {l: [[{b: 5}]]}
#Multi: {o: {b: int | *1, c: b + 1}, o: {d: 6}}
multi_decl: #Multi & {o: {b: 5}}
#Composed: {l: [{b: int | *1, c: b + 1}] & [{d: 6}]}
composed_decl: #Composed & {l: [{b: 5}]}
"#;
    let file = ostinato_lang::parse(source).expect("parse default narrowing");
    let value = ostinato_lang::eval_file(&file).expect("eval default narrowing");
    let json = value.to_json().expect("json");
    assert_eq!(json["simple"]["n"], 6);
    assert_eq!(json["simple"]["total"], 7);
    assert_eq!(json["disjunction"]["total"], 7);
    assert_eq!(json["typed"]["y"], 4);
    assert_eq!(json["list"]["l"], serde_json::json!([3, 4]));
    assert_eq!(json["two"]["sum"], 12);
    assert_eq!(json["optional"]["y"], 5);
    assert_eq!(json["string_value"]["y"], "xb");
    assert_eq!(json["direct"]["y"], 4);
    assert_eq!(json["nested"]["a"]["c"], 6);
    assert_eq!(json["nested"]["d"], 7);
    assert_eq!(json["list_nested"]["l"][0]["c"], 6);
    assert_eq!(json["list_nested"]["d"], 7);
    assert_eq!(json["substring"]["total"], 4);
    assert_eq!(json["pair"]["l"][0]["c"], 6);
    assert_eq!(json["pair"]["l"][1]["c"], 7);
    assert_eq!(json["nested_list"]["l"][0][0]["c"], 6);
    assert_eq!(json["nested_list"]["d"], 7);
    assert_eq!(json["multi_decl"]["o"]["c"], 6);
    assert_eq!(json["multi_decl"]["o"]["d"], 6);
    assert_eq!(json["composed_decl"]["l"][0]["c"], 6);
    assert_eq!(json["composed_decl"]["l"][0]["d"], 6);

    for source in [
        "package demo\n#L2: {l: [{b: int | *1, c: b + 1}]}\nx: #L2 & {l: [{b: 5}, {b: 6}]}\n",
        "package demo\n#L2: {l: [{b: int | *1, c: b + 1}]}\nx: #L2 & {l: []}\n",
        "package demo\n#P: {l: [{b: int | *1}, {d: 6}, ...]}\nx: #P & {l: [{b: 5}]}\n",
        "package demo\n#F: {l: [{b: int | *1}, {d: 6}]}\nx: #F & {l: [{b: 5}]}\n",
        "package demo\n#F: {l: [{b: int | *1}, {d: 6}]}\nx: #F & {l: [{b: 5}, {d: 6}, {e: 7}]}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse fixed-list arity");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "fixed-list arity must conflict: {source:?}"
        );
    }

    for source in [
        "package demo\n#D: {d: 6}\n#X: {o: {b: int | *1, c: b + 1}, o: #D}\nx: #X & {o: {b: 5}}\n",
        "package demo\n#D: {d: 6}\n#X: {o: {b: int | *1, c: b + 1}, o: #D}\nx: #X & {o: {b: 5, z: 9}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse closed multi-declaration");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "closed multi/composed declarations must reject extra fields: {source:?}"
        );
    }

    let open_multi = "package demo\n#X: {o: {b: int | *1, c: b + 1, ..., d: 6}, o: {e: 7}}\nx: #X & {o: {b: 5}}\n";
    let file = ostinato_lang::parse(open_multi).expect("parse open multi-declaration");
    let value = ostinato_lang::eval_file(&file).expect("eval open multi-declaration");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["o"]["c"], 6);
    assert_eq!(json["x"]["o"]["d"], 6);
    assert_eq!(json["x"]["o"]["e"], 7);

    let shadow =
        "package demo\n#A: {n: *1 | int, y: n + 1, inner: {#A: 5, v: #A}}\na: #A & {n: 3}\n";
    let file = ostinato_lang::parse(shadow).expect("parse shadowing narrowing");
    let value = ostinato_lang::eval_file(&file).expect("eval shadowing narrowing");
    let json = value.to_json().expect("json");
    assert_eq!(
        json["a"]["y"], 4,
        "shadowing narrowing recomputes dependents"
    );
    assert_eq!(json["a"]["inner"]["v"], 5);

    let definition_chain = "package demo\n#A: {n: *1 | int, y: n + 1}\n#B: #A & {n: 4}\nx: #B\n";
    let file = ostinato_lang::parse(definition_chain).expect("parse definition chain");
    let value = ostinato_lang::eval_file(&file).expect("eval definition chain");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["n"], 4);
    assert_eq!(json["x"]["y"], 5);

    let recursive = "package demo\n#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}\nx: #Node & {n: 3, children: [{n: 5}]}\n";
    let file = ostinato_lang::parse(recursive).expect("parse recursive narrowing");
    let value = ostinato_lang::eval_file(&file).expect("eval recursive narrowing");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["total"], 4);
    assert_eq!(json["x"]["children"][0]["total"], 6);
}

#[test]
fn split_composition_keeps_operand_lexical_scope() {
    let source = r#"
package demo
a: 2
x: {a: 1} & {b: a}
"#;
    let file = ostinato_lang::parse(source).expect("parse sibling lexical scope");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval sibling lexical scope")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 1);
    assert_eq!(json["x"]["b"], 2, "bare `a` sees the enclosing field");

    let defaults = "package demo\nx: {a: x.b} & {b: *1 | int} & {b: 2}\n";
    let file = ostinato_lang::parse(defaults).expect("parse selector default");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval selector default")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 2);
    assert_eq!(json["x"]["b"], 2);

    let cross = r#"
package demo
one: {a: one.b} & {b: 1, c: one.a}
two: {a: two.b} & {b: 1, c: two.d} & {d: two.a}
"#;
    let file = ostinato_lang::parse(cross).expect("parse cross-dependent operands");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval cross-dependent operands")
        .to_json()
        .expect("json");
    assert_eq!(json["one"], serde_json::json!({"a": 1, "b": 1, "c": 1}));
    assert_eq!(
        json["two"],
        serde_json::json!({"a": 1, "b": 1, "c": 1, "d": 1})
    );

    for source in [
        "package demo
x: {a: 1} & {b: a}
",
        "package demo
x: {#D: 1} & {v: #D}
",
        "package demo
x: {a: 1} & {c: x.a} & {b: c}
",
    ] {
        let file = ostinato_lang::parse(source).expect("parse composed lexical scope");
        let error = ostinato_lang::eval_file(&file)
            .expect_err("sibling operand fields are not bare identifiers");
        assert!(error.to_string().contains("undefined reference"), "{error}");
    }
}

#[test]
fn self_cycles_through_defaults_are_rejected() {
    for source in [
        "package demo\nx: {a: x.a + 1} & {a: *2 | int}\n",
        "package demo\nx: {a: x.a + 1} & {a: int | *2}\n",
        "package demo\nx: {a: x.a + 1} & {a: *2 | number}\n",
        "package demo\nx: {a: x.a + 1} & {a: *2 | *3}\n",
        "package demo\nlet d = *2 | int\nx: {a: x.a + 1} & {a: d}\n",
        "package demo\nbase: *2 | int\nx: {a: x.a + 1} & {a: base}\n",
        "package demo\n#D: *2 | int\nx: {a: x.a + 1} & {a: #D}\n",
        "package demo\nx: {a: \"\\(x.a)\"} & {a: *2 | int}\n",
        "package demo\nx: {o: {a: x.o.a + 1}} & {o: {a: *2 | int}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse self-cycle default");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "self-cycle through a default must not resolve: {source:?}"
        );
    }

    let transparent = "package demo\nx: {a: x.a} & {a: *2 | int}\n";
    let file = ostinato_lang::parse(transparent).expect("parse transparent self-selection");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval transparent self-selection")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 2);
}

#[test]
fn dependent_definition_fields_unify_with_injected_values() {
    let conflict = "package demo\n#D: {a: *1 | int, b: #D.a + 1}\nx: #D & {b: 9}\n";
    let file = ostinato_lang::parse(conflict).expect("parse dependent definition");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "a dependent definition field must conflict with an injected value"
    );

    let nested = "package demo\n#D: {a: *1 | int, b: {c: #D.a}, ...}\nx: #D & {b: {c: 9}}\n";
    let file = ostinato_lang::parse(nested).expect("parse nested dependent definition");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval nested dependent definition")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 1, "b": {"c": 9}}));
}

#[test]
fn recursive_bases_mutual_optionals_and_let_aliases_match_cue() {
    let recursive_base = r#"
package demo
#Node: {n: *1 | int, total: n + 1, child?: #Node}
#B: #Node & {n: 4}
x: #B & {child: {n: 5}}
"#;
    let file = ostinato_lang::parse(recursive_base).expect("parse recursive base");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval recursive base")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["n"], 4);
    assert_eq!(json["x"]["total"], 5);
    assert_eq!(json["x"]["child"]["total"], 6);

    let mutual = r#"
package demo
#A: {n?: #B | *{m: 1}}
#B: {m: *2 | int, w: m + 1}
x: #A & {n: {m: 5}}
"#;
    let file = ostinato_lang::parse(mutual).expect("parse mutual optional defaults");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval mutual optional defaults")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["n"]["w"], 6);

    let let_alias = "package demo\nlet X = f\nf: {a: 1, b: X.a}\nx: f\n";
    let file = ostinato_lang::parse(let_alias).expect("parse let alias self-selection");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval let alias self-selection")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["b"], 1);
}

#[test]
fn split_composition_self_selection_resolves_non_literal_operands() {
    let source = r#"
package demo
let base = {b: 4}
from_let: {a: from_let.b} & base
field_base: {b: 4}
from_field: {a: from_field.b} & field_base
reversed: field_base & {a: reversed.b}
self_field: {a: self_field.a} & {a: 1}
quoted: {a: quoted["b"]} & {b: 1}
let key = "b"
computed: {"\(key)": 1, a: computed["\(key)"]}
"#;
    let file = ostinato_lang::parse(source).expect("parse non-literal operands");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval non-literal operands")
        .to_json()
        .expect("json");
    assert_eq!(json["from_let"], serde_json::json!({"a": 4, "b": 4}));
    assert_eq!(json["from_field"], serde_json::json!({"a": 4, "b": 4}));
    assert_eq!(json["reversed"], serde_json::json!({"a": 4, "b": 4}));
    assert_eq!(json["self_field"], serde_json::json!({"a": 1}));
    assert_eq!(json["quoted"], serde_json::json!({"a": 1, "b": 1}));
    assert_eq!(json["computed"], serde_json::json!({"a": 1, "b": 1}));
}

#[test]
fn nested_comprehension_bodies_self_select_and_let_field_cycles_are_not_static() {
    let nested = "package demo
#M: {if true {if true {b: 5, y: #M.b}}}
x: #M
";
    let file = ostinato_lang::parse(nested).expect("parse nested comprehension");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval nested comprehension")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["y"], 5);

    let let_over_selector = "package demo
x: {b: 1, a: x.b, let Y = x.b, c: Y}
";
    let file = ostinato_lang::parse(let_over_selector).expect("parse let over selector");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval let over selector")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 1, "b": 1, "c": 1}));
}

#[test]
fn recursive_narrowing_materializes_deep_subtrees_quickly() {
    use std::time::{Duration, Instant};

    const DEPTH: usize = 200;
    let mut inner = "{n: 1}".to_owned();
    for index in 0..DEPTH {
        inner = format!("{{n: {}, children: [{}]}}", index + 2, inner);
    }
    let source = format!(
        "package demo
#Node: {{n: *1 | int, total: n + 1, children?: [...#Node]}}
x: #Node & {{children: [{inner}]}}
"
    );
    let file = ostinato_lang::parse(&source).expect("parse deep recursive narrowing");
    let started = Instant::now();
    let value = ostinato_lang::eval_file(&file).expect("eval deep recursive narrowing");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "deep recursive narrowing took {elapsed:?}"
    );
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["n"], 1);
    assert_eq!(json["x"]["total"], 2);
    let mut node = &json["x"];
    for _ in 0..=DEPTH {
        node = &node["children"][0];
    }
    assert_eq!(node["n"], 1);
    assert_eq!(node["total"], 2);
    assert_eq!(json["x"]["children"][0]["n"], DEPTH + 1);
    assert_eq!(json["x"]["children"][0]["total"], DEPTH + 2);
}

#[test]
fn recursive_injection_rejects_invalid_leaves() {
    for leaf in ["1", "null", "true", "\"a\"", "{children: [1]}"] {
        let source = format!(
            "package demo\n#Node: {{n: *1 | int, total: n + 1, children?: [...#Node]}}\nx: #Node & {{children: [{leaf}]}}\n"
        );
        let file = ostinato_lang::parse(&source).expect("parse invalid recursive leaf");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "recursive injection must reject leaf {leaf}"
        );
    }

    let union = "package demo\n#N: {child?: #N | int}\nx: #N & {child: 3}\n";
    let file = ostinato_lang::parse(union).expect("parse recursive union leaf");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval recursive union leaf")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["child"], 3);
}

#[test]
fn recursive_union_alternatives_materialize_in_either_order() {
    for source in [
        "package demo\n#N: {n: *1|int, total: n+1, child?: int | #N}\nx: #N & {child: {n: 5}}\n",
        "package demo\n#N: {n: *1|int, total: n+1, child?: #N | int}\nx: #N & {child: {n: 5}}\n",
        "package demo\n#N: {n: *1|int, total: n+1, child?: [...int] | #N}\nx: #N & {child: {n: 5}}\n",
        "package demo\n#N: {n: *1|int, total: n+1, children?: [...int] | [...#N]}\nx: #N & {children: [{n: 5}, ...]}\n",
        "package demo\n#N: {n: *1|int, total: n+1, children?: [...#N] | [...int]}\nx: #N & {children: [{n: 5}, ...]}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse recursive union");
        let json = ostinato_lang::eval_file(&file)
            .expect("eval recursive union")
            .to_json()
            .expect("json");
        let node = json["x"].get("child").unwrap_or(&json["x"]["children"][0]);
        assert_eq!(node["total"], 6);
    }

    for children in [
        "[null]",
        "[true]",
        "[\"s\"]",
        "[[1]]",
        "[null, ...]",
        "[{n: 2}, 3, ...]",
        "[1, {n: 5}]",
    ] {
        let source = format!(
            "package demo\n#N: {{n: *1|int, total: n+1, children?: [...int] | [...#N]}}\nx: #N & {{children: {children}}}\n"
        );
        let file = ostinato_lang::parse(&source).expect("parse invalid disjunction leaf");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "disjunction constraint must reject {children}"
        );
    }

    let shape = "package demo\n#N: {n: *1|int, total: n+1, child?: int | [...#N]}\nx: #N & {child: {n: 5}}\n";
    let file = ostinato_lang::parse(shape).expect("parse invalid disjunction shape");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "struct must not satisfy int | [...#N]"
    );

    let scalar =
        "package demo\n#N: {n: *1|int, total: n+1, child?: int | #N}\nx: #N & {child: 3}\n";
    let file = ostinato_lang::parse(scalar).expect("parse recursive union scalar");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval recursive union scalar")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["child"], 3);
}

#[test]
fn transparent_let_alias_chains_resolve_in_three_operand_splits() {
    let source = "package demo\nlet X = f\nf: {a: X.b} & {b: X.c} & {c: 1}\nx: f\n";
    let file = ostinato_lang::parse(source).expect("parse let alias chain");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval let alias chain")
        .to_json()
        .expect("json");
    assert_eq!(json["f"], serde_json::json!({"a": 1, "b": 1, "c": 1}));

    let multi = "package demo\nlet X = f\nf: {a: X.b, d: X.c} & {b: 1, c: X.a}\nx: f\n";
    let file = ostinato_lang::parse(multi).expect("parse let alias multi-dependent");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval let alias multi-dependent")
        .to_json()
        .expect("json");
    assert_eq!(
        json["f"],
        serde_json::json!({"a": 1, "b": 1, "c": 1, "d": 1})
    );
}

#[test]
fn recursive_injection_validates_composite_shapes() {
    for source in [
        "package demo\n#N: {n: *1|int, total: n+1, children?: [...#N]}\nx: #N & {children: [[1]]}\n",
        "package demo\n#N: {n: *1|int, total: n+1, children?: [...#N]}\nx: #N & {children: {n: 5}}\n",
        "package demo\n#N: {n: *1|int, total: n+1, child?: #N}\nx: #N & {child: [1]}\n",
        "package demo\n#N: {n: *1|int, total: n+1, children?: [...#N]}\nx: #N & {children: [{children: [[1]]}]}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse invalid recursive shape");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "recursive shape must be rejected: {source:?}"
        );
    }

    let required = "package demo\n#N: {n: *1|int, total: n+1, children: [...#N]}\nx: #N & {children: [{n: 5}, {n: 6}]}\n";
    let file = ostinato_lang::parse(required).expect("parse required recursive list");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval required recursive list")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["children"][0]["total"], 6);
    assert_eq!(json["x"]["children"][1]["total"], 7);

    let prefix = "package demo\n#N: {n: *1|int, total: n+1, children?: [int, ...#N]}\nx: #N & {children: [1, {n: 2}]}\n";
    let file = ostinato_lang::parse(prefix).expect("parse prefix recursive list");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval prefix recursive list")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["children"][1]["total"], 3);
}

#[test]
fn recursive_injection_materializes_open_lists() {
    let plain = "package demo\n#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}\nx: #Node & {children: [{n: 5}, ...]}\n";
    let file = ostinato_lang::parse(plain).expect("parse open-list recursion");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval open-list recursion")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["children"][0]["total"], 6);

    let base = "package demo\n#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}\n#B: #Node & {n: 4}\nx: #B & {children: [{n: 5}, ...]}\n";
    let file = ostinato_lang::parse(base).expect("parse base open-list recursion");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval base open-list recursion")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["total"], 5);
    assert_eq!(json["x"]["children"][0]["total"], 6);
}

#[test]
fn field_mediated_let_cycles_resolve_in_split_forms() {
    for source in [
        "package demo\nlet X = f\nf: {a: X.b} & {b: 1}\nx: f\n",
        "package demo\nlet X = f\nf: {a: 1} & {b: X.a}\nx: f\n",
        "package demo\nlet X = f\nf: {a: X.b} & {b: *1 | int}\nx: f\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse split let cycle");
        let json = ostinato_lang::eval_file(&file)
            .expect("eval split let cycle")
            .to_json()
            .expect("json");
        assert_eq!(json["f"], serde_json::json!({"a": 1, "b": 1}));
        assert_eq!(json["x"], serde_json::json!({"a": 1, "b": 1}));
    }
}

#[test]
fn multi_dependent_split_operands_reach_a_fixpoint() {
    let source = "package demo\nx: {a: x.b, e: x.c} & {b: 1, c: x.a}\n";
    let file = ostinato_lang::parse(source).expect("parse multi-dependent operands");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval multi-dependent operands")
        .to_json()
        .expect("json");
    assert_eq!(
        json["x"],
        serde_json::json!({"a": 1, "b": 1, "c": 1, "e": 1})
    );

    let optional = "package demo\nx: {a: x.a} & {a?: *2 | int}\n";
    let file = ostinato_lang::parse(optional).expect("parse optional self-selection");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval optional self-selection")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 2);
}

#[test]
fn nested_cross_dependent_split_operands_match_cue() {
    let source = r#"
package demo
x: {o: {a: x.o.b}} & {o: {b: 1, c: x.o.a}}
y: {o: {a: y.o.b}} & {o: {b: 2, c: y.o.a}} & {o: {d: y.o.c}}
z: {o: {p: {a: z.o.p.b}}} & {o: {p: {b: 3, c: z.o.p.a}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse nested split operands");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval nested split operands")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["o"]["a"], 1);
    assert_eq!(json["x"]["o"]["c"], 1);
    assert_eq!(json["y"]["o"]["c"], 2);
    assert_eq!(json["y"]["o"]["d"], 2);
    assert_eq!(json["z"]["o"]["p"]["a"], 3);
    assert_eq!(json["z"]["o"]["p"]["c"], 3);
}

#[test]
fn nested_selector_self_selection_matches_cue() {
    let source = r#"
package demo
x: {o: {a: x.o.b, b: 1}}
#D: {o: {a: #D.o.b, b: 1}}
d: #D
#Deep: {o: {p: {a: #Deep.o.p.b, b: 2}}}
deep: #Deep
"#;
    let file = ostinato_lang::parse(source).expect("parse nested self selection");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval nested self selection")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["o"]["a"], 1);
    assert_eq!(json["d"]["o"]["a"], 1);
    assert_eq!(json["deep"]["o"]["p"]["a"], 2);
}

#[test]
fn self_selecting_non_literal_split_bases_match_cue() {
    let source = r#"
package demo
let base = {b: 1, c: x.a}
x: {a: x.b} & base
field_base: {b: 2, c: z.a}
z: {a: z.b} & field_base
let nested_base = {o: {b: 3, c: w.o.a}}
w: {o: {a: w.o.b}} & nested_base
let list_base = {l: [4, v.a]}
v: {a: v.l[0]} & list_base
"#;
    let file = ostinato_lang::parse(source).expect("parse non-literal split base");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval non-literal split base")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 1);
    assert_eq!(json["x"]["c"], 1);
    assert_eq!(json["z"]["a"], 2);
    assert_eq!(json["field_base"]["c"], 2);
    assert_eq!(json["w"]["o"]["a"], 3);
    assert_eq!(json["w"]["o"]["c"], 3);
    assert_eq!(json["v"]["a"], 4);
    assert_eq!(json["v"]["l"], serde_json::json!([4, 4]));
}

#[test]
fn disjunction_alternatives_recompute_dependent_fields_when_narrowed() {
    let source = r#"
package demo
x: ({a: *1 | int, b: a} | {a: string}) & {a: 3}
y: ({a: string} | {a: *1 | int, b: a}) & {a: 4}
z: ({a: *1 | int, b: a} | {a: *2 | int, b: a}) & {a: 5}
w: ({a: *1 | int, b: a, c: b + 1} | {a: string}) & {a: 6}
n: ({o: {a: *1 | int, b: a}} | {o: {a: string}}) & {o: {a: 7}}
l: ({a: *1 | int, b: [a]} | {a: string}) & {a: 8}
"#;
    let file = ostinato_lang::parse(source).expect("parse disjunction narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval disjunction narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 3, "b": 3}));
    assert_eq!(json["y"], serde_json::json!({"a": 4, "b": 4}));
    assert_eq!(json["z"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["w"], serde_json::json!({"a": 6, "b": 6, "c": 7}));
    assert_eq!(json["n"]["o"], serde_json::json!({"a": 7, "b": 7}));
    assert_eq!(json["l"]["b"], serde_json::json!([8]));
}

#[test]
fn self_selection_respects_narrowed_defaults() {
    let source = r#"
package demo
flat: {a: *1 | int, b: a, c: flat.b} & {a: 6}
nested: {o: {a: *1 | int, b: a, c: nested.o.b}} & {o: {a: 6}}
"#;
    let file = ostinato_lang::parse(source).expect("parse narrowed self selection");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval narrowed self selection")
        .to_json()
        .expect("json");
    assert_eq!(json["flat"], serde_json::json!({"a": 6, "b": 6, "c": 6}));
    assert_eq!(
        json["nested"]["o"],
        serde_json::json!({"a": 6, "b": 6, "c": 6})
    );

    // A non-transparent self-default reference must fail closed instead of
    // reading the injected default back as a concrete value.
    for source in [
        "package demo\nflat: {a: flat.a + 1} & {a: *2 | int}\n",
        "package demo\nx: {o: {a: x.o.a + 1}} & {o: {a: *2 | int}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse self-default cycle");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "self-default cycle must not resolve: {source:?}"
        );
    }
}

#[test]
fn narrowed_selector_chains_and_cross_operand_projection_match_cue() {
    let source = r#"
package demo
c1: {a: c1.b, b: c1.c, c: *1 | int} & {c: 3}
c2: {a: c2.b, b: c2.c, c: int} & {c: 3}
c3: {o: {a: *1 | int, b: a}} & {o: {a: 5}, c: c3.o.b}
c4: {a: *1 | int, b: a} & {a: 5, c: c4.b}
let V = 3
c5: {a: *1 | int, c: *1 | int, z: c5.c} & {a: 5, c: V}
"#;
    let file = ostinato_lang::parse(source).expect("parse narrowed selector chain");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval narrowed selector chain")
        .to_json()
        .expect("json");
    assert_eq!(json["c1"], serde_json::json!({"a": 3, "b": 3, "c": 3}));
    assert_eq!(json["c2"], serde_json::json!({"a": 3, "b": 3, "c": 3}));
    assert_eq!(json["c3"]["o"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["c3"]["c"], 5);
    assert_eq!(json["c4"], serde_json::json!({"a": 5, "b": 5, "c": 5}));
    assert_eq!(json["c5"], serde_json::json!({"a": 5, "c": 3, "z": 3}));
}

#[test]
fn selector_paths_through_list_indices_propagate_narrowing() {
    let source = r#"
package demo
x: {l: [{a: *1 | int, b: a}]} & {l: [{a: 5}], c: x.l[0].b}
deep: {l: [{o: {a: *1 | int, b: a}}]} & {l: [{o: {a: 5}}], c: deep.l[0].o.b}
deep2: {l: [[{a: *1 | int, b: a}]]} & {l: [[{a: 5}]], c: deep2.l[0][0].b}
openl: {l: [{a: *1 | int, b: a}, ...]} & {l: [{a: 5}], c: openl.l[0].b}
wrapped: {l: [{a: *1 | int, b: a}]} & {l: [{a: 5}], c: wrapped.l[0].b * 2}
interp: {l: [{a: *1 | int, b: a}]} & {l: [{a: 5}], c: "\(interp.l[0].b)"}
#D: {y: *2 | int, z: y}
defs: {l: [{x: #D}]} & {l: [{x: {y: 6}}], c: defs.l[0].x.z}
"#;
    let file = ostinato_lang::parse(source).expect("parse list selector narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval list selector narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["c"], 5);
    assert_eq!(json["x"]["l"][0]["b"], 5);
    assert_eq!(json["deep"]["c"], 5);
    assert_eq!(json["deep2"]["c"], 5);
    assert_eq!(json["openl"]["c"], 5);
    assert_eq!(json["wrapped"]["c"], 10);
    assert_eq!(json["interp"]["c"], "5");
    assert_eq!(json["defs"]["c"], 6);
}

#[test]
fn optional_operand_narrowing_recomputes_dependents_and_rejects_conflicts() {
    let source = r#"
package demo
x: {a: *1 | int, b: a} & {a?: 5}
y: {a: *1 | int, b: a, c: y.b} & {a?: 5}
nested: {o: {a: *1 | int, b: a}} & {o: {a?: 5}}
let V = 6
let_form: {a: *1 | int, b: a} & {a?: V}
#D: {a: *1 | int, b: a}
def_form: #D & {a?: 7}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval optional narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["y"], serde_json::json!({"a": 5, "b": 5, "c": 5}));
    assert_eq!(json["nested"]["o"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["let_form"], serde_json::json!({"a": 6, "b": 6}));
    assert_eq!(json["def_form"], serde_json::json!({"a": 7, "b": 7}));

    for source in [
        "package demo\nx: {a: *1 | int, b: a} & {a?: 5, b?: 7}\n",
        "package demo\nx: {o: {a: *1 | int, b: a}} & {o: {a?: 5, b?: 7}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse optional conflict");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional conflict must fail: {source:?}"
        );
    }
}

#[test]
fn disjunction_alternatives_recompute_nested_list_dependents() {
    let source = r#"
package demo
lx: ({l: [{a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a: 3}]}
nested: ({l: [[{a: *1 | int, b: a}]]} | {l: [[{a: string}]]}) & {l: [[{a: 4}]]}
listcomp: ({l: [for i in [1] {a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a: 5}]}
"#;
    let file = ostinato_lang::parse(source).expect("parse list disjunction narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval list disjunction narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["lx"]["l"][0], serde_json::json!({"a": 3, "b": 3}));
    assert_eq!(
        json["nested"]["l"][0][0],
        serde_json::json!({"a": 4, "b": 4})
    );
    assert_eq!(
        json["listcomp"]["l"][0],
        serde_json::json!({"a": 5, "b": 5})
    );
}

#[test]
fn disjunction_alternatives_inject_optional_narrowing_into_elements() {
    let source = r#"
package demo
let P = {a: *1 | int, b: a}
nested: ({o: {a: *1 | int, b: a}} | {o: {a: string}}) & {o: {a?: 3}}
list: ({l: [{a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 3}]}
list2: ({l: [[{a: *1 | int, b: a}]]} | {l: [[{a: string}]]}) & {l: [[{a?: 3}]]}
listcomp: ({l: [for i in [1] {a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 4}]}
let_alt: (P | {a: string}) & {a?: 5}
#D: {a: *1 | int, b: a}
def_alt: (#D | {a: string}) & {a?: 6}
open: ({l: [{a: *1 | int, b: a}, ...]} | {l: [{a: string}]}) & {l: [{a?: 7}]}
deep: ({o: {p: {a: *1 | int, b: a}}} | {o: {p: {a: string}}}) & {o: {p: {a?: 8}}}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional disjunction elements");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval optional disjunction elements")
        .to_json()
        .expect("json");
    assert_eq!(json["nested"]["o"], serde_json::json!({"a": 3, "b": 3}));
    assert_eq!(json["list"]["l"][0], serde_json::json!({"a": 3, "b": 3}));
    assert_eq!(
        json["list2"]["l"][0][0],
        serde_json::json!({"a": 3, "b": 3})
    );
    assert_eq!(
        json["listcomp"]["l"][0],
        serde_json::json!({"a": 4, "b": 4})
    );
    assert_eq!(json["let_alt"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["def_alt"], serde_json::json!({"a": 6, "b": 6}));
    assert_eq!(json["open"]["l"][0], serde_json::json!({"a": 7, "b": 7}));
    assert_eq!(json["deep"]["o"]["p"], serde_json::json!({"a": 8, "b": 8}));

    // Multi-element optional disjunction lists currently fail closed.
    for source in [
        "package demo\nx: ({l: [{a: *1 | int, b: a}, {a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 3}, {a?: 4}]}\n",
        "package demo\nx: ({l: [{a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 3}], c: x.l[0].b}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse optional disjunction fail-closed");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional disjunction shape must fail closed: {source:?}"
        );
    }
}

#[test]
fn let_mediated_self_default_cycles_fail_closed() {
    for source in [
        "package demo\nlet X = x.a + 0\nx: {a: X + 1} & {a: *2 | int}\n",
        "package demo\nlet X = x.a\nx: {a: X + 1} & {a: *2 | int}\n",
        "package demo\nlet X = x.o.a + 0\nx: {o: {a: X + 1}} & {o: {a: *2 | int}}\n",
        "package demo\nlet X = x.l[0].a + 0\nx: {l: [{a: X + 1}]} & {l: [{a: *2 | int}]}\n",
        "package demo\nlet X = #D.a + 0\n#D: {a: X + 1}\nx: #D\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse let-mediated self-default");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "let-mediated self-default must fail closed: {source:?}"
        );
    }
}

#[test]
fn optional_narrowing_keeps_optional_only_fields_omitted() {
    let source = r#"
package demo
only: {a?: *1 | int} & {a?: 5}
list_opt: {l: [{a: *1 | int, b: a}]} & {l: [{a?: 5}], c: list_opt.l[0].b}
deep_opt: {l: [{o: {a: *1 | int, b: a}}]} & {l: [{o: {a?: 5}}], c: deep_opt.l[0].o.b}
open_opt: {l: [{a: *1 | int, b: a}, ...]} & {l: [{a?: 5}], c: open_opt.l[0].b}
listcomp_opt: {l: [for i in [1] {a: *1 | int, b: a}]} & {l: [{a?: 5}]}
#D: {y: *2 | int, z: y}
def_list_opt: {l: [{x: #D}]} & {l: [{x: {y?: 6}}], c: def_list_opt.l[0].x.z}
self_opt: {a: self_opt.b} & {b: *1 | int} & {b?: 5}
"#;
    let file = ostinato_lang::parse(source).expect("parse optional-only narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval optional-only narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["only"], serde_json::json!({}));
    assert_eq!(json["list_opt"]["c"], 5);
    assert_eq!(json["list_opt"]["l"][0]["b"], 5);
    assert_eq!(json["deep_opt"]["c"], 5);
    assert_eq!(json["open_opt"]["c"], 5);
    assert_eq!(json["listcomp_opt"]["l"][0]["b"], 5);
    assert_eq!(json["def_list_opt"]["c"], 6);
    assert_eq!(json["def_list_opt"]["l"][0]["x"]["z"], 6);
    assert_eq!(json["self_opt"], serde_json::json!({"a": 5, "b": 5}));

    // Optional-only narrowing must not materialize or legalize references.
    for source in [
        "package demo\nx: {a?: *1 | int, b: a} & {a?: 5}\n",
        "package demo\nx: {o: {a?: *1 | int, b: a}} & {o: {a?: 5}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse optional-only reference");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "optional-only reference must fail: {source:?}"
        );
    }
}

#[test]
fn comprehension_generated_fields_see_external_narrowing() {
    let source = r#"
package demo
x: {if true {a: *1 | int, b: a}} & {a: 5}
y: {l: [for i in [1] {a: *1 | int, b: a}]} & {l: [{a: 6}]}
z: {l: [for i in [1] {a: int, b: a}]} & {l: [{a: 7}]}
"#;
    let file = ostinato_lang::parse(source).expect("parse comprehension narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval comprehension narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 5, "b": 5}));
    assert_eq!(json["y"]["l"][0]["b"], 6);
    assert_eq!(json["z"]["l"][0]["b"], 7);
}

#[test]
fn cross_field_mutual_selection_cycles_fail_closed() {
    for source in [
        "package demo\nx: {a: y.b, b: 1}\ny: {b: x.a}\n",
        "package demo\nlet X = f\nlet Y = g\nf: {a: Y.b, b: 1}\ng: {b: X.a}\nx: f\n",
        "package demo\nx: {a: y.b, b: 1}\ny: {b: z.c, c: 1}\nz: {c: x.a}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse cross-field cycle");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "cross-field selection cycle must fail closed: {source:?}"
        );
    }
}

#[test]
fn disjunction_alternatives_through_aliases_and_optional_values() {
    let source = r#"
package demo
let P = {a: *1 | int, b: a}
let_alias: (P | {a: string}) & {a: 9}
#P: {a: *1 | int, b: a}
def_alt: (#P | {a: string}) & {a: 4}
opt: ({a: *1 | int, b: a} | {a: string}) & {a?: 3}
"#;
    let file = ostinato_lang::parse(source).expect("parse alias disjunction narrowing");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval alias disjunction narrowing")
        .to_json()
        .expect("json");
    assert_eq!(json["let_alias"], serde_json::json!({"a": 9, "b": 9}));
    assert_eq!(json["def_alt"], serde_json::json!({"a": 4, "b": 4}));
    assert_eq!(json["opt"], serde_json::json!({"a": 3, "b": 3}));
}

#[test]
fn definition_self_selection_through_a_let_alias_resolves() {
    let source = r#"
package demo
let X = #D
#D: {a: X.b, b: 1}
x: #D
"#;
    let file = ostinato_lang::parse(source).expect("parse definition let alias");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval definition let alias")
        .to_json()
        .expect("json");
    assert_eq!(json["x"], serde_json::json!({"a": 1, "b": 1}));
}

#[test]
fn mutual_field_mediated_let_cycles_match_cue() {
    let source = r#"
package demo
let X = f
let Y = g
f: {a: Y.b, b: 1}
g: {b: X.b}
x: f
let A = h
let B = k
let C = m
h: {a: B.b, b: 1}
k: {b: C.c, c: 2}
m: {c: A.b}
y: h
let Z = z_self
z_self: {a: Z.a} & {a: 1}
z_out: z_self
"#;
    let file = ostinato_lang::parse(source).expect("parse mutual let cycles");
    let json = ostinato_lang::eval_file(&file)
        .expect("eval mutual let cycles")
        .to_json()
        .expect("json");
    assert_eq!(json["f"], serde_json::json!({"a": 1, "b": 1}));
    assert_eq!(json["g"], serde_json::json!({"b": 1}));
    assert_eq!(json["x"], serde_json::json!({"a": 1, "b": 1}));
    assert_eq!(json["h"], serde_json::json!({"a": 1, "b": 1}));
    assert_eq!(json["k"], serde_json::json!({"b": 1, "c": 2}));
    assert_eq!(json["m"], serde_json::json!({"c": 1}));
    assert_eq!(json["y"], serde_json::json!({"a": 1, "b": 1}));
    assert_eq!(json["z_out"], serde_json::json!({"a": 1}));
}

#[test]
fn mutually_recursive_open_lists_rematerialize_quickly() {
    use std::time::{Duration, Instant};

    let source = r#"
package demo
#A: {bs: [...#B], u: len(bs)}
#B: {as: [...#A], t: len(as)}
x: #A & {bs: [{as: [{}]}]}
z: #A & {bs: x.bs}
"#;
    let file = ostinato_lang::parse(source).expect("parse mutual open lists");
    let started = Instant::now();
    let value = ostinato_lang::eval_file(&file).expect("eval mutual open lists");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "mutual open-list re-materialization took {elapsed:?}"
    );
    let json = value.to_json().expect("json");
    let expected = serde_json::json!({
        "bs": [{"as": [{"bs": [], "u": 0}], "t": 1}],
        "u": 1
    });
    assert_eq!(json["x"], expected);
    assert_eq!(json["z"], expected);
}

#[test]
fn selection_and_narrowing_scope_rows_match_cue() {
    let source = r#"
package demo
#A: {n: *1 | int, m: n + 1}
#B: #A & {n: 4}
definition_chain: #B
#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}
recursive: #Node & {n: 3, children: [{n: 2}]}
#Shadow: {n: *1 | int, y: n + 1, inner: {#Shadow: 5, v: #Shadow}}
shadow: #Shadow & {n: 3}
split: {a: split.b} & {b: 1}
#P: {env: string, greeting: "hello \(env)"}
if true {#P: {env: "prod"}}
merged: #P
#M: {b?: int, if true {b: 5, y: #M.b}}
body_local: #M
"#;
    let file = ostinato_lang::parse(source).expect("parse selection rows");
    let value = ostinato_lang::eval_file(&file).expect("eval selection rows");
    let json = value.to_json().expect("json");
    assert_eq!(json["definition_chain"]["n"], 4);
    assert_eq!(json["definition_chain"]["m"], 5);
    assert_eq!(json["recursive"]["total"], 4);
    assert_eq!(json["recursive"]["children"][0]["total"], 3);
    assert_eq!(json["shadow"]["y"], 4);
    assert_eq!(json["shadow"]["inner"]["v"], 5);
    assert_eq!(json["split"]["a"], 1);
    assert_eq!(json["split"]["b"], 1);
    assert_eq!(json["merged"]["env"], "prod");
    assert_eq!(json["merged"]["greeting"], "hello prod");
    assert_eq!(json["body_local"]["b"], 5);
    assert_eq!(json["body_local"]["y"], 5);

    for source in [
        "package demo\nx: {if true {b: 1}, a: b}\n",
        "package demo\nif true {b: 1}\na: b\n",
        "package demo\nx: {if true {b: 1}, y: {a: b}}\n",
    ] {
        let file = ostinato_lang::parse(source).expect("parse bare generated label");
        let error = ostinato_lang::eval_file(&file).expect_err("bare generated label must fail");
        assert!(error.to_string().contains("undefined reference"), "{error}");
    }

    let selected = "package demo\nx: {if true {b: 1}, a: x.b}\n";
    let file = ostinato_lang::parse(selected).expect("parse generated selector");
    let json = ostinato_lang::eval_file(&file)
        .expect("selector reaches generated label")
        .to_json()
        .expect("json");
    assert_eq!(json["x"]["a"], 1);

    let let_cycle = "package demo\nlet X = f\nf: X\nx: f\n";
    let file = ostinato_lang::parse(let_cycle).expect("parse let cycle");
    let value = ostinato_lang::eval_file(&file).expect("let cycle is incomplete, not an error");
    assert!(
        value.to_json().is_none(),
        "field-mediated let cycle stays incomplete"
    );

    let direct = "package demo\nlet X = Y\nlet Y = X\nx: X\n";
    let file = ostinato_lang::parse(direct).expect("parse direct let cycle");
    assert!(ostinato_lang::eval_file(&file).is_err());
}

#[test]
fn gateway_n_of_and_expression_modes_compile() {
    let nof = r#"
package demo
workflow: {
    name: "nof"
    steps: {
        a: {kind: "noop"}
        b: {kind: "noop"}
        q: {kind: "gateway", needs: ["a", "b"], gateway: {join: "n_of", n: 2}}
    }
}
"#;
    let compiled = ostinato_lang::compile(nof).expect("compile n_of");
    let gateway = compiled
        .spec
        .step("q")
        .and_then(|step| step.gateway.as_ref())
        .expect("gateway");
    assert_eq!(gateway.join, Some(JoinMode::NOf));
    assert_eq!(gateway.n, Some(2));

    let expression = r#"
package demo
workflow: {
    name: "expression"
    steps: {
        a: {kind: "noop"}
        b: {kind: "noop"}
        m: {kind: "gateway", needs: ["a", "b"], gateway: {
            join: "expression"
            labels: {left: "a", right: "b"}
            expression: "edges.left && !edges.right"
        }}
    }
}
"#;
    let compiled = ostinato_lang::compile(expression).expect("compile expression");
    let gateway = compiled
        .spec
        .step("m")
        .and_then(|step| step.gateway.as_ref())
        .expect("gateway");
    assert_eq!(gateway.join, Some(JoinMode::Expression));
    assert_eq!(gateway.labels["left"], "a");
}

#[test]
fn gateway_n_of_and_expression_validation_is_path_qualified() {
    for (source, needle) in [
        (
            r#"a: {kind:"gateway", needs:["b"], gateway:{join:"n_of"}}
               b: {kind:"noop"}"#,
            "workflow.steps.a.gateway.n is required",
        ),
        (
            r#"a: {kind:"gateway", needs:["b"], gateway:{join:"n_of", n:2}}
               b: {kind:"noop"}"#,
            "workflow.steps.a.gateway.n must be between 1 and 1",
        ),
        (
            r#"a: {kind:"gateway", needs:["b"], gateway:{join:"or", n:1}}
               b: {kind:"noop"}"#,
            "`n` is only allowed for `n_of`",
        ),
        (
            r#"a: {kind:"gateway", needs:["b"], gateway:{join:"expression", expression:"edges.c"}}
               b: {kind:"noop"}"#,
            "workflow.steps.a.gateway.expression is invalid",
        ),
    ] {
        let source =
            format!("package demo\nworkflow: {{ name: \"bad\"\n steps: {{ {source} }} }}\n");
        let error = compile_error(&source);
        assert!(error.contains(needle), "{error}");
    }
}

#[test]
fn gateway_n_of_duplicate_needs_and_non_injective_labels_are_rejected() {
    for (source, needle) in [
        (
            r#"q: {kind:"gateway", needs:["a","a"], gateway:{join:"n_of", n:1}}
               a: {kind:"noop"}"#,
            "duplicate `a`",
        ),
        (
            r#"q: {kind:"gateway", needs:["a","b"], gateway:{
                   join:"expression", labels:{x:"a", y:"a", z:"b"}, expression:"edges.x"
               }}
               a: {kind:"noop"}
               b: {kind:"noop"}"#,
            "maps multiple labels",
        ),
        (
            r#"q: {kind:"gateway", needs:["a"], gateway:{
                   join:"expression", labels:{and:"a"}, expression:"edges.and"
               }}
               a: {kind:"noop"}"#,
            "must match",
        ),
    ] {
        let source =
            format!("package demo\nworkflow: {{ name: \"bad\"\n steps: {{ {source} }} }}\n");
        let error = compile_error(&source);
        assert!(error.contains(needle), "{error}");
    }
}

#[test]
fn nested_multiline_literals_inside_interpolation_match_cue() {
    let source = r#"
package demo
a: "x\("""
hi
""")y"
b: "x\('''
hi
''')y"
c: """
x\("""
hi
""")y
"""
d: """
x\('''
hi
''')y
"""
e: '''
x\("""
hi
""")y
'''
f: '''
x\('''
hi
''')y
'''
g: '''
x\('''
  v=\(1+2)
  ''')y
'''
h: "x\('a)b')y"
i: "x\('a"b')y"
j: """
x\('''
"""
''')y
"""
"#;
    let file = ostinato_lang::parse(source).expect("parse nested multiline interpolation");
    let value = ostinato_lang::eval_file(&file).expect("eval nested multiline interpolation");
    let json = value.to_json().expect("json");
    assert_eq!(json["a"], "xhiy");
    assert_eq!(json["b"], "xhiy");
    assert_eq!(json["c"], "xhiy");
    assert_eq!(json["d"], "xhiy");
    assert_eq!(json["e"], "eGhpeQ==");
    assert_eq!(json["f"], "eGhpeQ==");
    assert_eq!(json["g"], "eHY9M3k=");
    assert_eq!(json["h"], "xa)by");
    assert_eq!(json["i"], "xa\"by");
    assert_eq!(json["j"], "x\"\"\"y");

    for source in [
        "package demo\na: \"x\\(\"\"\"\nhi\n\"\"\"\n\"\n",
        "package demo\na: '''\nx\\('''\nhi\n'''\n'''\n",
    ] {
        assert!(
            ostinato_lang::parse(source).is_err(),
            "unterminated or malformed nested literal must fail: {source:?}"
        );
    }
}

#[test]
fn bytes_literal_field_labels_match_string_labels() {
    let source = r#"
package demo
plain: {'k': 1}
nested: {'a': {'b': 2}}
unicode: {'日': 3}
multiline: {'''
k
''': 4}
#D: {'k'?: int}
typed: #D & {'k': 5}
invalid_utf8: {'\xff': 6}
escaped: {'\x6b': 7}
"#;
    let file = ostinato_lang::parse(source).expect("parse bytes labels");
    let value = ostinato_lang::eval_file(&file).expect("eval bytes labels");
    let json = value.to_json().expect("json");
    assert_eq!(json["plain"]["k"], 1);
    assert_eq!(json["nested"]["a"]["b"], 2);
    assert_eq!(json["unicode"]["日"], 3);
    assert_eq!(json["multiline"]["k"], 4);
    assert_eq!(json["typed"]["k"], 5);
    assert_eq!(json["invalid_utf8"]["�"], 6);
    assert_eq!(json["escaped"]["k"], 7);

    let comprehensions = r#"
package demo
list_comp: [for i in [1, 2] {'k': i}]
list_comp_if: [for i in [1] if true {'k': i}]
list_comp_optional: [for i in [1] {'k'?: int}]
list_comp_def: [for i in [1] {'#D': i}]
list_comp_multiline: [for i in [1] {
	'k': i
}]
struct_comp: {for i in [1] {'k': i}}
"#;
    let file = ostinato_lang::parse(comprehensions).expect("parse bytes-label comprehensions");
    let value = ostinato_lang::eval_file(&file).expect("eval bytes-label comprehensions");
    let json = value.to_json().expect("json");
    assert_eq!(json["list_comp"][0]["k"], 1);
    assert_eq!(json["list_comp"][1]["k"], 2);
    assert_eq!(json["list_comp_if"][0]["k"], 1);
    assert_eq!(json["list_comp_multiline"][0]["k"], 1);
    assert_eq!(json["struct_comp"]["k"], 1);

    // Raw bytes labels keep their own identity. JSON export renders them as
    // U+FFFD, but a valid U+FFFD string label is a different field.
    let raw = "package demo\nx: {'\\xff': 1}\n";
    let file = ostinato_lang::parse(raw).expect("parse raw label");
    let value = ostinato_lang::eval_file(&file).expect("eval raw label");
    assert_eq!(value.to_json().expect("json")["x"]["�"], 1);

    let control = "package demo\nx: {'\\a': 1}\n";
    let file = ostinato_lang::parse(control).expect("parse control label");
    let value = ostinato_lang::eval_file(&file).expect("eval control label");
    assert_eq!(value.to_json().expect("json")["x"]["\u{7}"], 1);

    let mixed = "package demo\nx: {'\\xff': 1, '\\x07ff': 2}\n";
    let file = ostinato_lang::parse(mixed).expect("parse mixed labels");
    let value = ostinato_lang::eval_file(&file).expect("eval mixed labels");
    let json = value.to_json().expect("json");
    assert_eq!(json["x"]["�"], 1);
    assert_eq!(json["x"]["\u{7}ff"], 2);

    let escaped_prefix = "package demo\nx: {\"\\u0007ff\": 1}\n";
    let file = ostinato_lang::parse(escaped_prefix).expect("parse escaped-prefix label");
    let value = ostinato_lang::eval_file(&file).expect("eval escaped-prefix label");
    assert_eq!(value.to_json().expect("json")["x"]["\u{7}ff"], 1);

    let unselectable = "package demo\nx: {'\\xff': 1}\ny: x[\"�\"]\n";
    let file = ostinato_lang::parse(unselectable).expect("parse raw selection");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "raw bytes labels are not selectable by their rendered character"
    );

    let distinct = "package demo\nx: {'\\xff': 1, \"�\": 2}\n";
    let file = ostinato_lang::parse(distinct).expect("parse distinct labels");
    let value = ostinato_lang::eval_file(&file).expect("eval distinct labels");
    assert!(
        value.to_json().is_none(),
        "two distinct labels that render as U+FFFD cannot be exported losslessly"
    );

    let control_selection =
        "package demo\nx: {\"\\u0007ff\": 1}\nsel: x.\"\\u0007ff\"\nkeys: [for k, v in x {k}]\n";
    let file = ostinato_lang::parse(control_selection).expect("parse control selection");
    let control_value = ostinato_lang::eval_file(&file).expect("eval control selection");
    let json = control_value.to_json().expect("json");
    assert_eq!(json["sel"], 1);
    assert_eq!(json["keys"][0], "\u{7}ff");

    let bell_selection =
        "package demo\nx: {'\\a': 1}\nsel: x[\"\\a\"]\nkeys: [for k, v in x {k}]\n";
    let file = ostinato_lang::parse(bell_selection).expect("parse bell selection");
    let value = ostinato_lang::eval_file(&file).expect("eval bell selection");
    let json = value.to_json().expect("json");
    assert_eq!(json["sel"], 1);
    assert_eq!(json["keys"][0], "\u{7}");
    assert_eq!(
        ostinato_lang::select_path(&control_value, "x.\"\\u0007ff\"")
            .expect("select_path control")
            .to_json(),
        Some(serde_json::json!(1))
    );

    let selection = r#"
package demo
x: {'k': 1}
dot: x.k
index: x["k"]
"#;
    let file = ostinato_lang::parse(selection).expect("parse bytes-label selection");
    let value = ostinato_lang::eval_file(&file).expect("eval bytes-label selection");
    let json = value.to_json().expect("json");
    assert_eq!(json["dot"], 1);
    assert_eq!(json["index"], 1);

    let duplicate = "package demo\nx: {\"k\": 1, 'k': 2}\n";
    let file = ostinato_lang::parse(duplicate).expect("parse duplicate labels");
    assert!(
        ostinato_lang::eval_file(&file).is_err(),
        "quoted string and bytes labels must share identity"
    );

    let interpolated = "package demo\nx: {'\\(1)': 2}\n";
    assert!(
        ostinato_lang::parse(interpolated).is_err(),
        "bytes labels cannot interpolate"
    );
}

#[test]
fn tag_placement_rules_match_cue() {
    // Allowed: identifier/quoted labels, single-rune bytes labels, definitions,
    // nested structs, and a tag on the field that holds a list.
    let allowed = r#"
package demo
ident: string | *"i" @tag(ident)
quoted: string | *"q" @tag(quoted)
single: {'a': string | *"s" @tag(single)}
single_unicode: {'日': string | *"u" @tag(unicode)}
single_invalid: {'\xff': string | *"v" @tag(invalid)}
empty: {'': string | *"e" @tag(empty)}
unicode_escape: {'\u00e9': string | *"u" @tag(escap)}
#D: {a: string @tag(def_field)}
def_value: #D & {a: "d"}
nested: {a: string | *"n" @tag(nested_field)}
list_holder: [{a: string | *"l"}] @tag(list_holder)
dynamic_ok: {k: "a", "\(k)": {x: string | *"d"}}
pattern_newline_attr: {[string]: string | *"ok"
@tag(standalone)}
pattern_leading_attr: {
	@tag(standalone)
	[string]: 1
}
pattern_between_attrs: {
	[string]: 1
	@tag(standalone)
	[string]: int
}
pattern_listcomp_leading: {
	p: [for i in [1] {
		@tag(standalone)
		[string]: int
	}]
}
list_leading_field: [{
	@tag(note)
	a: 1
	b: 2
}]
comp_leading_field: {for i in [1] {
	@tag(note)
	a: 1
}}
"#;
    let file = ostinato_lang::parse(allowed).expect("parse allowed tags");
    let value = ostinato_lang::eval_file(&file).expect("eval allowed tags");
    let json = value.to_json().expect("json");
    assert_eq!(json["ident"], "i");
    assert_eq!(json["single"]["a"], "s");
    assert_eq!(json["def_value"]["a"], "d");
    assert_eq!(json["unicode_escape"]["é"], "u");
    assert_eq!(json["dynamic_ok"]["a"]["x"], "d");
    assert_eq!(json["pattern_newline_attr"], serde_json::json!({}));
    assert_eq!(json["pattern_leading_attr"], serde_json::json!({}));
    assert_eq!(json["pattern_between_attrs"], serde_json::json!({}));
    assert_eq!(
        json["pattern_listcomp_leading"]["p"],
        serde_json::json!([{}])
    );
    assert_eq!(json["list_leading_field"][0]["a"], 1);
    assert_eq!(json["comp_leading_field"]["a"], 1);

    // Rejected: multi-rune bytes labels and any tag lexically inside a list.
    for source in [
        "package demo\nparams: {x: {'ab': string | *\"v\" @tag(ab)}}\n",
        "package demo\nparams: {x: {'日本': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {x: {'\\xff\\xff': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {x: {'\\xc3\\xa9': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {x: {'\\xe6\\x97\\xa5': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {x: {'\\303\\251': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {x: {'''\na\n''': string | *\"v\" @tag(v)}}\n",
        "package demo\nparams: {for i in [1] {\"k\": string | *\"v\" @tag(k)}}\n",
        "package demo\nparams: {if true {a: string | *\"v\" @tag(a)}}\n",
        "package demo\nparams: {for i in [\"a\"] {#D: {x: string | *\"v\" @tag(d)}}}\n",
        "package demo\nparams: {let L = {for i in [1] {\"k\": string | *\"v\" @tag(k)}}, x: L}\n",
        "package demo\nparams: {for k, v in {k: {a: string | *\"v\" @tag(src)}} {b: string | *\"z\"}}\n",
        "package demo\nparams: {[string]: {p: string | *\"v\" @tag(p)}}\n",
        "package demo\nparams: {x: {[string]: string | *\"v\" @tag(p)}}\n",
        "package demo\nparams: {x: {[string]: {a: 1} @tag(p)}}\n",
        "package demo\nparams: {x: {for i in [1] {[string]: string | *\"v\" @tag(p)}}}\n",
        "package demo\nparams: {[string]: {#D: {p: string | *\"v\" @tag(p)}}}\n",
        "package demo\nparams: {k: \"a\", \"\\(k)\": string | *\"v\" @tag(t)}\n",
        "package demo\nparams: {k: \"a\", \"\\(k)\": {x: string | *\"v\" @tag(t)}}\n",
        "package demo\nparams: {@tag(x) a: string | *\"v\"}\n",
        "package demo\nparams: {@tag(x) [string]: int}\n",
        "package demo\nparams: {@tag(x) @tag(y)}\n",
        "package demo\n#D: {'ab': string @tag(ab)}\nx: #D\n",
        "package demo\nparams: {l: [{a: string | *\"v\" @tag(a)}]}\n",
        "package demo\nparams: {l: [{\"a\": string | *\"v\" @tag(a)}]}\n",
        "package demo\nparams: {l: [...{a: string | *\"v\" @tag(a)}]}\n",
        "package demo\nparams: {l: [for i in [1] {a: string | *\"v\" @tag(a)}]}\n",
        "package demo\nparams: {l: [{outer: {a: string | *\"v\" @tag(a)}}]}\n",
        "package demo\nparams: {v: [{#D: {a: string | *\"v\" @tag(a)}}]}\n",
        "package demo\nparams: {v: [for i in [{a: string | *\"v\" @tag(a)}] {i.a}]}\n",
        "package demo\nparams: {host?: string @tag(host)}\n",
        "package demo\n#P: {host?: string @tag(host)}\nparams: #P\n",
        "package demo\nparams: {cfg?: {host: string @tag(host)}}\n",
        "package demo\n#P: {cfg?: {host: string @tag(host)}}\nparams: #P\n",
    ] {
        let outcome = ostinato_lang::parse(source).and_then(|file| ostinato_lang::eval_file(&file));
        let error = outcome.expect_err("tag placement must fail");
        assert!(
            !error.to_string().is_empty(),
            "unexpected empty error: {error}"
        );
    }

    // Standalone own-line attributes are declaration metadata, not field tags.
    let standalone = "package demo\nparams: {\n@tag(x)\na: string | *\"v\"\n}\n";
    let file = ostinato_lang::parse(standalone).expect("parse standalone attribute");
    let tags = ostinato_lang::parameter_tags(&file).expect("collect tags");
    assert!(
        !tags.contains_key("x"),
        "standalone @tag must not register as a field tag"
    );
}

#[test]
fn tag_discovery_traverses_lists_and_optional_references() {
    let source = r#"
package demo
#E: {n: int | *2 @tag(n, type=int), m: n * 3}
#P: {ls?: [#E], open?: [...#E], maybe?: #E}
params: {ls: [#E], open: [...#E] & [#E], maybe: #E}
"#;
    let file = ostinato_lang::parse(source).expect("parse list tag discovery");
    let tags = ostinato_lang::parameter_tags(&file).expect("collect list tags");
    assert!(
        tags.contains_key("n"),
        "list element tags must be discovered"
    );

    let standalone = r#"
package demo
#E: {host: string @tag(host)}
#P: {e?: #E}
params: #P & {e: {host: "x"}}
"#;
    let file = ostinato_lang::parse(standalone).expect("parse optional definition reference");
    let tags = ostinato_lang::parameter_tags(&file).expect("collect optional reference tags");
    assert!(
        tags.contains_key("host"),
        "standalone definition tags referenced from optional fields must be discovered"
    );
}

#[test]
fn unused_imports_are_rejected_like_cue() {
    let validate = |source: &str| {
        let file = ostinato_lang::parse(source).expect("parse import fixture");
        ostinato_lang::scope::validate_imports(&file)
    };

    for source in [
        "package demo\nimport \"strings\"\nx: 1\n",
        "package demo\nimport s \"strings\"\nx: 1\n",
        "package demo\nimport _ \"strings\"\nx: 1\n",
        "package demo\nimport \"strings\"\nx: {strings: 1, y: strings}\n",
        "package demo\nimport \"strings\"\nx: {strings?: 1, y: strings}\n",
        "package demo\nimport \"strings\"\nx: [for strings in [1] {strings}]\n",
        "package demo\nimport \"strings\"\nx: [for strings in [[1]] for b in strings {b}]\n",
        "package demo\nimport \"strings\"\nx: [for strings in [[1]] if len(strings) == 1 for b in strings {b}]\n",
        "package demo\nimport params \"strings\"\nx: 1\n",
        "package demo\nimport \"strings\"\nimport \"strings\"\nx: strings.ToUpper(\"a\")\n",
        "package demo\nimport a \"strings\"\nimport a \"strings\"\nx: a.ToUpper(\"a\")\n",
    ] {
        let error = validate(source).expect_err("unused import must be rejected");
        assert!(
            error.to_string().contains("imported and not used")
                || error
                    .to_string()
                    .contains("redeclared as imported package name"),
            "unexpected error: {error}"
        );
    }

    for source in [
        "package demo\nimport \"strings\"\nx: strings.ToUpper(\"a\")\n",
        "package demo\nimport s \"strings\"\nx: s.ToUpper(\"a\")\n",
        "package demo\nimport \"strings\"\n_x: strings.ToUpper(\"a\")\ny: 1\n",
        "package demo\nimport \"strings\"\n#D: strings.ToUpper(\"a\")\ny: 1\n",
        "package demo\nimport \"strings\"\nx: \"\\(strings.ToUpper(\"a\"))\"\n",
        "package demo\nimport \"strings\"\nx: {(strings.ToLower(\"K\")): 1}\n",
        "package demo\nimport \"strings\"\nx: {if strings.ToUpper(\"a\") == \"A\" {y: 1}}\n",
        "package demo\nimport params \"strings\"\nx: params.ToUpper(\"a\")\n",
        "package demo\nimport \"strings\"\nx: [for s in [[\"a\"]] for b in s {strings.ToUpper(b)}]\n",
    ] {
        assert!(validate(source).is_ok(), "used import rejected: {source:?}");
    }

    let redeclared = "package demo\nimport \"strings\"\nstrings: 1\nx: 1\n";
    let error = validate(redeclared).expect_err("field/import redeclaration must fail");
    assert!(
        error
            .to_string()
            .contains("redeclared as imported package name"),
        "unexpected error: {error}"
    );

    let comp_shadow =
        "package demo\nimport \"strings\"\nx: [for strings in [[1]] for b in strings {b}]\n";
    let error = validate(comp_shadow).expect_err("shadowed import must be unused");
    assert!(
        !error.to_string().contains("__ost_"),
        "unused-import error leaked an internal alias: {error}"
    );

    let unused = ostinato_lang::parse("package demo\nimport \"strings\"\nx: 1\n").expect("parse");
    assert!(
        ostinato_lang::eval_file(&unused).is_err(),
        "eval_file must validate imports"
    );
}

#[test]
fn compiles_await_and_emit_steps() {
    let source = r#"
package demo

workflow: {
    name: "events"
    steps: {
        emit: {
            kind: "noop"
            emit: [{
                name: "ready"
                payload: {value: 1}
            }, {
                name: "public"
                external: true
                payload: {value: 2}
            }]
        }
        wait: {
            kind: "await"
            needs: ["emit"]
            await: {event: "ready", source: "internal", timeoutMs: 100}
        }
    }
}
"#;
    let compiled = compile(source).expect("compile await/emit");
    let emit = &compiled.spec.steps["emit"];
    assert_eq!(emit.emit.len(), 2);
    assert!(!emit.emit[0].external);
    assert!(emit.emit[1].external);
    let wait = &compiled.spec.steps["wait"];
    assert_eq!(wait.kind, StepKind::Await);
    let await_spec = wait.await_spec.as_ref().expect("await spec");
    assert_eq!(await_spec.event, "ready");
    assert_eq!(await_spec.timeout_ms, Some(100));
}

#[test]
fn await_rejects_same_group_internal_emitters() {
    let source = r#"
package demo

workflow: {
    name: "same-group"
    steps: {
        emit: {
            kind: "noop"
            emit: [{name: "ready"}]
        }
        wait: {
            kind: "await"
            await: {event: "ready"}
        }
    }
}
"#;
    let error = compile_error(source);
    assert!(
        error.contains("strictly earlier group"),
        "unexpected: {error}"
    );
}

#[test]
fn await_rejects_retry_exec_and_unknown_fields() {
    let retry = r#"
package demo
workflow: {
    name: "bad"
    steps: {
        wait: {
            kind: "await"
            await: {event: "ready"}
            retry: {max: 1}
        }
    }
}
"#;
    assert!(compile_error(retry).contains("retry"));
    let exec = r#"
package demo
workflow: {
    name: "bad"
    steps: {
        wait: {
            kind: "await"
            await: {event: "ready"}
            exec: {cmd: ["true"]}
        }
    }
}
"#;
    assert!(compile_error(exec).contains("exec"));
    let unknown = r#"
package demo
workflow: {
    name: "bad"
    steps: {
        wait: {
            kind: "await"
            await: {event: "ready", typo: true}
        }
    }
}
"#;
    assert!(compile_error(unknown).contains("unknown field"));
}

#[test]
fn conditions_can_reference_durable_events() {
    let source = r#"
package demo

workflow: {
    name: "event-condition"
    steps: {
        emit: {
            kind: "exec"
            exec: {cmd: ["true"]}
            emit: [{name: "ready", external: true, payload: {ok: true}}]
        }
        gate: {
            kind: "gateway"
            needs: ["emit"]
            gateway: {
                split: "xor"
                branches: {
                    yes: "events.ready.payload.ok == true"
                    no:  true
                }
                default: "no"
            }
        }
        yes: {kind: "noop"}
        no:  {kind: "noop"}
    }
}
"#;
    compile(source).expect("events are valid condition roots");
}

#[test]
fn value_from_json_keeps_big_integers_and_large_exponents() {
    let big: serde_json::Value =
        serde_json::from_str("123456789012345678901234567890").expect("json");
    let value = ostinato_lang::value_from_json(&big);
    assert!(
        matches!(value, ostinato_lang::Value::BigInt(_)),
        "{value:?}"
    );
    assert_eq!(
        value.to_json().expect("json").to_string(),
        "123456789012345678901234567890"
    );

    let huge: serde_json::Value = serde_json::from_str("1e400").expect("json");
    let value = ostinato_lang::value_from_json(&huge);
    assert!(matches!(value, ostinato_lang::Value::Float(_)), "{value:?}");
    assert!(value.to_json().is_some());
}

#[test]
fn unify_merges_structs_and_rejects_conflicts() {
    let left = ostinato_lang::value_from_json(&json!({"a": 1, "nested": {"x": 1}}));
    let right = ostinato_lang::value_from_json(&json!({"b": 2, "nested": {"y": 2}}));
    let merged = ostinato_lang::unify(&left, &right).expect("unify");
    assert_eq!(
        merged.to_json().expect("json"),
        json!({"a": 1, "b": 2, "nested": {"x": 1, "y": 2}})
    );
    let conflict = ostinato_lang::unify(
        &ostinato_lang::value_from_json(&json!({"a": 1})),
        &ostinato_lang::value_from_json(&json!({"a": 2})),
    )
    .expect_err("conflict");
    assert!(conflict.to_string().contains("conflicting") || conflict.is_conflict());
}

#[test]
fn numeric_unification_uses_value_equality_but_keeps_int_float_types() {
    let unify = |left: &str, right: &str| {
        let left: serde_json::Value = serde_json::from_str(left).expect("json");
        let right: serde_json::Value = serde_json::from_str(right).expect("json");
        ostinato_lang::unify(
            &ostinato_lang::value_from_json(&left),
            &ostinato_lang::value_from_json(&right),
        )
    };
    for (left, right) in [("-0", "0"), ("0e0", "0.0"), ("1e2", "100.0")] {
        assert!(unify(left, right).is_ok(), "{left} vs {right}");
    }
    assert!(unify("1", "1.0").is_err());
    assert!(unify("1", "2").is_err());
}

#[test]
fn hyperbolic_boundary_values_remain_finite_without_the_oracle() {
    let source = r#"
package demo
import "math"
a: math.Sinh(709.7)
b: math.Cosh(709.437)
c: math.Sinh(-709.78)
d: math.Cosh(709.78)
"#;
    let file = ostinato_lang::parse(source).expect("parse");
    let mut imports = BTreeMap::new();
    imports.insert(
        "math".to_owned(),
        ostinato_lang::builtin_module("math").expect("math builtin"),
    );
    let value =
        ostinato_lang::eval_file_with_imports(&file, &imports, &BTreeMap::new()).expect("eval");
    let json = value.to_json().expect("json");
    for field in ["a", "b", "c", "d"] {
        assert!(
            json[field].as_f64().is_some_and(f64::is_finite),
            "{field} should be finite: {}",
            json[field]
        );
    }
    assert_eq!(json["a"].as_f64(), Some(8.274920138401322e307));
    assert_eq!(json["b"].as_f64(), Some(6.361277807292747e307));
    assert_eq!(json["c"].as_f64(), Some(-8.964113971972578e307));
    assert_eq!(json["d"].as_f64(), Some(8.964113971972578e307));
}

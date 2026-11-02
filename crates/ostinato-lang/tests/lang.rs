#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::StepKind;
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
#P: {name: string @tag(name)}
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
#W: {}
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
    assert!(ostinato_lang::eval_file(&file).is_err());
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

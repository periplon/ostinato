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
    assert!(ostinato_lang::eval_file(&file).is_err());
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
        "false && (1 | 2)",
        "true || (1 | 2)",
    ] {
        let source = format!("package demo\na: {expression}\n");
        let file = ostinato_lang::parse(&source).expect("parse");
        assert!(
            ostinato_lang::eval_file(&file).is_err(),
            "expected unresolved error for {expression}"
        );
    }
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
    assert!(ostinato_lang::eval_file(&file).is_err());

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
    assert!(ostinato_lang::eval_file(&file).is_err());
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
    assert!(ostinato_lang::eval_file(&file).is_err());
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

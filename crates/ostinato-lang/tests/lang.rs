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
    assert_eq!(json["b"].to_string(), "1000.0");
    assert_eq!(json["c"].to_string(), "150.0");
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

#![allow(clippy::expect_used)]

use ostinato_domain::{AgentProvider, IsolationMode, PromptDelivery};
use ostinato_lang::compile;

const BASE: &str = r#"
package demo

agents: {
    reviewer: {
        provider: "claude"
        model: "claude-test"
        system: "review strictly"
        prompt: "default prompt"
        command: ["fake-claude", "--flag"]
        promptDelivery: "stdin"
        env: {TEAM: "core"}
        timeoutMs: 1234
        isolation: {mode: "worktree", base: "HEAD", keep: true}
        artifacts: ["reports/**/*.md", "out.txt"]
    }
}

workflow: {
    name: "agents"
    steps: {
        review: {
            kind: "agent"
            agent: {
                use: "reviewer"
                prompt: "Review {{ params.goal }}"
                model: "override"
                isolation: "none"
                artifacts: "only/*.txt"
                env: {TEAM: "override", EXTRA: "1"}
            }
        }
    }
}
"#;

#[test]
fn definitions_are_merged_with_step_overrides() {
    let compiled = compile(BASE).expect("compile");
    let agent = compiled
        .spec
        .step("review")
        .expect("review")
        .agent
        .as_ref()
        .expect("agent");
    assert_eq!(agent.definition.as_deref(), Some("reviewer"));
    assert_eq!(agent.provider, AgentProvider::Claude);
    assert_eq!(agent.model.as_deref(), Some("override"));
    assert_eq!(agent.system.as_deref(), Some("review strictly"));
    assert_eq!(agent.prompt, "Review {{ params.goal }}");
    assert_eq!(
        agent.command.as_deref(),
        Some(&["fake-claude".to_owned(), "--flag".to_owned()][..])
    );
    assert_eq!(agent.prompt_delivery, Some(PromptDelivery::Stdin));
    assert_eq!(agent.env.get("TEAM").map(String::as_str), Some("override"));
    assert_eq!(agent.env.get("EXTRA").map(String::as_str), Some("1"));
    assert_eq!(agent.timeout_ms, Some(1234));
    assert_eq!(
        agent.isolation.as_ref().expect("isolation").mode,
        IsolationMode::None
    );
    assert_eq!(agent.artifacts, vec!["only/*.txt".to_owned()]);
}

#[test]
fn definition_prompt_and_string_shorthand_are_supported() {
    let source = r#"
package demo
agents: helper: {prompt: "from definition"}
workflow: {
    name: "shorthand"
    steps: {
        work: {kind: "agent", agent: "helper"}
    }
}
"#;
    let compiled = compile(source).expect("compile");
    let agent = compiled
        .spec
        .step("work")
        .expect("work")
        .agent
        .as_ref()
        .expect("agent");
    assert_eq!(agent.prompt, "from definition");
    assert_eq!(agent.definition.as_deref(), Some("helper"));
    assert_eq!(agent.provider, AgentProvider::Command);
}

#[test]
fn unreferenced_definitions_are_still_validated() {
    let source = r#"
package demo
agents: broken: {provider: "unknown"}
workflow: {name: "x", steps: {a: {kind: "noop"}}}
"#;
    let error = compile(source).expect_err("must reject provider");
    assert!(error.to_string().contains("unknown provider"), "{error}");
}

#[test]
fn string_command_is_shell_but_argv_is_preserved() {
    let source = r#"
package demo
agents: shell: {command: "fake-agent --flag"}
workflow: {
    name: "shell"
    steps: {a: {kind: "agent", agent: {use: "shell", prompt: "hi"}}}
}
"#;
    let compiled = compile(source).expect("compile");
    let agent = compiled
        .spec
        .step("a")
        .expect("a")
        .agent
        .as_ref()
        .expect("agent");
    assert_eq!(
        agent.command.as_deref(),
        Some(
            &[
                "sh".to_owned(),
                "-c".to_owned(),
                "fake-agent --flag".to_owned()
            ][..]
        )
    );
}

#[test]
fn invalid_agent_schemas_are_rejected() {
    let cases = [
        (r#"agents: {a: {provider: "nope"}}"#, "unknown provider"),
        (
            r#"agents: {a: {isolation: "container"}}"#,
            "unknown isolation mode",
        ),
        (
            r#"agents: {a: {isolation: {mode: "worktree", base: "-b"}}}"#,
            "must not start with",
        ),
        (
            r#"agents: {a: {isolation: {mode: "none", keep: true}}}"#,
            "only meaningful",
        ),
        (
            r#"agents: {a: {artifacts: "/etc/passwd"}}"#,
            "must be relative",
        ),
        (
            r#"agents: {a: {artifacts: "../secret"}}"#,
            "must not contain",
        ),
        (
            r#"agents: {a: {artifacts: ".git/config"}}"#,
            "internal directories",
        ),
        (
            r#"agents: {a: {env: {LD_PRELOAD: "x"}}}"#,
            "reserved or unsafe",
        ),
        (
            r#"agents: {a: {env: {OSTINATO_AGENT: "x"}}}"#,
            "reserved or unsafe",
        ),
        (
            r#"agents: {a: {command: "echo hi", promptDelivery: "arg"}}"#,
            "cannot be used with a shell-string",
        ),
        (r#"agents: {a: {timeoutMs: 0}}"#, "must be >= 1"),
        (r#"agents: {a: {unknown: 1}}"#, "unknown field"),
        (r#"agents: {a: {command: []}}"#, "must not be empty"),
        (
            r#"agents: {a: {}}
workflow: {name: "x", steps: {s: {kind: "agent", agent: {use: "missing", prompt: "x"}}}}"#,
            "unknown agent definition",
        ),
        (
            r#"agents: {a: {}}
workflow: {name: "x", steps: {s: {kind: "agent", agent: {use: "a"}}}}"#,
            "has no prompt",
        ),
        (
            r#"workflow: {name: "x", steps: {s: {kind: "agent", agent: {}}}}"#,
            "prompt is required",
        ),
        (
            r#"workflow: {name: "x", steps: {s: {kind: "exec", agent: {prompt: "x"}, exec: {cmd: ["true"]}}}}"#,
            "only allowed when kind",
        ),
    ];
    for (declaration, expected) in cases {
        let source = if declaration.contains("workflow:") {
            format!("package demo\n{declaration}\n")
        } else {
            format!(
                "package demo\n{declaration}\nworkflow: {{name: \"x\", steps: {{s: {{kind: \"noop\"}}}}}}\n"
            )
        };
        let error = compile(&source).expect_err(expected);
        assert!(
            error.to_string().contains(expected),
            "expected `{expected}` for `{declaration}`, got: {error}"
        );
    }
}

#[test]
fn agent_spec_serialization_stays_compatible_for_inline_steps() {
    let source = r#"
package demo
workflow: {
    name: "legacy"
    steps: {a: {kind: "agent", agent: {prompt: "hello"}}}
}
"#;
    let compiled = compile(source).expect("compile");
    let agent = compiled
        .spec
        .step("a")
        .expect("a")
        .agent
        .as_ref()
        .expect("agent");
    let value = serde_json::to_value(agent).expect("serialize");
    let object = value.as_object().expect("object");
    assert_eq!(
        object.len(),
        4,
        "only the four legacy fields may serialize: {object:?}"
    );
    for key in ["prompt", "system", "model", "command"] {
        assert!(object.contains_key(key), "missing {key}");
    }
}

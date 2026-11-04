use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ostinato_application::{
    AgentOutcome, AgentRequest, AgentRunner, CommandExecutor, ExecOutcome, ExecRequest, PortError,
    PortResult,
};
use ostinato_domain::{AgentProvider, PromptDelivery, sha256_hex};
use wait_timeout::ChildExt;

const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

/// Variables that must not leak into an agent from the engine's environment.
const DANGEROUS_INHERITED: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_SSH_COMMAND",
    "PROMPT_COMMAND",
    "NODE_OPTIONS",
    "PYTHONSTARTUP",
    "PERL5OPT",
    "RUBYOPT",
];

/// Executes deterministic commands with bounded timeouts and captured output.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessExecutor;

impl CommandExecutor for ProcessExecutor {
    fn execute(&self, request: &ExecRequest) -> PortResult<ExecOutcome> {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .envs(&request.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|err| {
            PortError::Executor(format!("cannot spawn `{}`: {err}", request.program))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Executor("stdout pipe unavailable".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PortError::Executor("stderr pipe unavailable".to_owned()))?;
        let stdout_rx = spawn_reader(stdout);
        let stderr_rx = spawn_reader(stderr);

        let status = match request.timeout {
            Some(timeout) => child.wait_timeout(timeout).map_err(|err| {
                PortError::Executor(format!("wait failed for `{}`: {err}", request.program))
            })?,
            None => Some(child.wait().map_err(|err| {
                PortError::Executor(format!("wait failed for `{}`: {err}", request.program))
            })?),
        };
        let timed_out = status.is_none();
        let exit_code = status.and_then(|status| status.code()).unwrap_or(-1);
        // Reap descendants even on success: a background daemon that inherited
        // the pipes must not extend the step beyond its timeout.
        kill_group(&mut child);
        let (stdout, _) = drain_reader(&stdout_rx);
        let (stderr, _) = drain_reader(&stderr_rx);
        Ok(ExecOutcome {
            exit_code,
            stdout,
            stderr,
            timed_out,
        })
    }
}

fn spawn_reader(mut stream: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if sender.send(chunk[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    receiver
}

fn drain_reader(receiver: &Receiver<Vec<u8>>) -> (String, bool) {
    let mut buffer = Vec::new();
    let mut truncated = false;
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            truncated = true;
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(chunk) => {
                if buffer.len() < MAX_CAPTURE_BYTES {
                    let take = chunk.len().min(MAX_CAPTURE_BYTES - buffer.len());
                    buffer.extend_from_slice(&chunk[..take]);
                    if take < chunk.len() {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                // A descendant kept the pipe open. Keep what was already read
                // instead of discarding the agent's output.
                truncated = true;
                break;
            }
        }
    }
    if truncated {
        buffer.extend_from_slice(b"\n[output truncated]");
    }
    (String::from_utf8_lossy(&buffer).into_owned(), truncated)
}

#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    let pid = child.id();
    // Negative pid targets the process group created above. `kill` is used
    // instead of `unsafe` libc calls (the workspace forbids unsafe code).
    let kill = if Path::new("/bin/kill").exists() {
        "/bin/kill"
    } else {
        "kill"
    };
    let _ = Command::new(kill)
        .args(["-KILL", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_group(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug)]
struct Invocation {
    argv: Vec<String>,
    delivery: PromptDelivery,
}

/// Runs an agent command, feeding the prompt on stdin or as the final argument.
#[derive(Debug, Clone)]
pub struct CommandAgent {
    command: Option<Vec<String>>,
    timeout: std::time::Duration,
}

impl CommandAgent {
    #[must_use]
    pub fn new(command: Option<String>) -> Self {
        Self {
            command: command.map(|command| vec!["sh".to_owned(), "-c".to_owned(), command]),
            timeout: std::time::Duration::from_secs(300),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn invocation(&self, request: &AgentRequest) -> PortResult<Invocation> {
        let requested_delivery = request.prompt_delivery;
        let provider_delivery = match request.provider {
            AgentProvider::Codex | AgentProvider::Opencode => PromptDelivery::Arg,
            AgentProvider::Command | AgentProvider::Claude => PromptDelivery::Stdin,
        };
        let (mut argv, default_delivery) = if let Some(command) = &request.command {
            (command.clone(), provider_delivery)
        } else {
            match request.provider {
                AgentProvider::Command => (
                    self.command.clone().ok_or_else(|| {
                        PortError::Config(
                            "agent step requires `agent.command`, a provider, \
                             `--agent-cmd` or `OSTINATO_AGENT`"
                                .to_owned(),
                        )
                    })?,
                    PromptDelivery::Stdin,
                ),
                AgentProvider::Claude => {
                    let mut argv = vec![
                        "claude".to_owned(),
                        "-p".to_owned(),
                        "--output-format".to_owned(),
                        "stream-json".to_owned(),
                        "--verbose".to_owned(),
                    ];
                    if let Some(model) = &request.model {
                        argv.push("--model".to_owned());
                        argv.push(model.clone());
                    }
                    (argv, PromptDelivery::Stdin)
                }
                AgentProvider::Codex => {
                    let mut argv = vec![
                        "codex".to_owned(),
                        "exec".to_owned(),
                        "--json".to_owned(),
                        "--skip-git-repo-check".to_owned(),
                        "--sandbox".to_owned(),
                        "workspace-write".to_owned(),
                    ];
                    if let Some(model) = &request.model {
                        argv.push("--model".to_owned());
                        argv.push(model.clone());
                    }
                    (argv, PromptDelivery::Arg)
                }
                AgentProvider::Opencode => {
                    let mut argv = vec![
                        "opencode".to_owned(),
                        "run".to_owned(),
                        "--format".to_owned(),
                        "json".to_owned(),
                    ];
                    if let Some(model) = &request.model {
                        argv.push("--model".to_owned());
                        argv.push(model.clone());
                    }
                    (argv, PromptDelivery::Arg)
                }
            }
        };
        if argv.is_empty() {
            return Err(PortError::Config("empty agent command".to_owned()));
        }
        let shell = is_shell_command(&argv);
        if shell && requested_delivery == Some(PromptDelivery::Arg) {
            return Err(PortError::Config(
                "promptDelivery `arg` cannot be used with a shell-string command; \
                 use argv form or `stdin`"
                    .to_owned(),
            ));
        }
        let delivery = if shell {
            PromptDelivery::Stdin
        } else {
            requested_delivery.unwrap_or(default_delivery)
        };
        if delivery == PromptDelivery::Arg {
            argv.push(String::new());
        }
        Ok(Invocation { argv, delivery })
    }
}

fn is_shell_command(argv: &[String]) -> bool {
    argv.len() == 3
        && argv[1] == "-c"
        && matches!(argv[0].as_str(), "sh" | "bash" | "zsh" | "dash" | "ksh")
}

impl Default for CommandAgent {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AgentRunner for CommandAgent {
    fn run(&self, request: &AgentRequest) -> PortResult<AgentOutcome> {
        let invocation = self.invocation(request)?;
        let prompt = match &request.system {
            Some(system) => format!("SYSTEM:\n{system}\n\nUSER:\n{}", request.prompt),
            None => request.prompt.clone(),
        };
        let mut argv = invocation.argv;
        if invocation.delivery == PromptDelivery::Arg
            && let Some(last) = argv.last_mut()
        {
            last.clone_from(&prompt);
        }
        if invocation.delivery == PromptDelivery::Arg && prompt.starts_with('-') {
            return Err(PortError::Config(
                "promptDelivery `arg` refuses a prompt that starts with `-`; it would be parsed as a CLI option (use stdin or a command wrapper)".to_owned(),
            ));
        }
        let stdin_sha256 = if invocation.delivery == PromptDelivery::Stdin {
            Some(sha256_hex(prompt.as_bytes()))
        } else {
            None
        };
        let Some(program) = argv.first().cloned() else {
            return Err(PortError::Config("empty agent command".to_owned()));
        };
        let args = argv[1..].to_vec();

        let mut env = BTreeMap::new();
        for (key, value) in &request.env {
            env.insert(key.clone(), value.clone());
        }
        env.insert("OSTINATO_STEP".to_owned(), request.step.clone());
        env.insert("OSTINATO_RUN_ID".to_owned(), request.run_id.clone());
        env.insert(
            "OSTINATO_MODEL".to_owned(),
            request.model.clone().unwrap_or_default(),
        );
        env.insert(
            "OSTINATO_PROVIDER".to_owned(),
            request.provider.as_str().to_owned(),
        );
        env.insert(
            "OSTINATO_AGENT_NAME".to_owned(),
            request.definition.clone().unwrap_or_default(),
        );
        let env_keys: Vec<String> = env.keys().cloned().collect();
        let env_hashes: BTreeMap<String, String> = env
            .iter()
            .map(|(key, value)| (key.clone(), sha256_hex(value.as_bytes())))
            .collect();

        let mut process = Command::new(&program);
        for key in DANGEROUS_INHERITED {
            process.env_remove(key);
        }
        process
            .args(&args)
            .envs(&env)
            .stdin(if invocation.delivery == PromptDelivery::Stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &request.cwd {
            process.current_dir(cwd);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.process_group(0);
        }
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(err) => {
                return Ok(AgentOutcome {
                    text: String::new(),
                    stdout: String::new(),
                    stderr: String::new(),
                    provider: request.provider.as_str().to_owned(),
                    argv,
                    exit_code: None,
                    timed_out: false,
                    success: false,
                    error: Some(format!("cannot spawn agent `{program}`: {err}")),
                    signal: None,
                    pid: None,
                    env_keys,
                    env_hashes,
                    stdout_truncated: false,
                    stderr_truncated: false,
                    stdin_sha256: stdin_sha256.clone(),
                });
            }
        };
        let pid = child.id();

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Agent("agent stdout pipe unavailable".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PortError::Agent("agent stderr pipe unavailable".to_owned()))?;
        let stdout_rx = spawn_reader(stdout);
        let stderr_rx = spawn_reader(stderr);

        // Write stdin from a separate thread: an agent that writes output
        // before reading stdin must not deadlock before the timeout applies.
        let stdin_writer = if invocation.delivery == PromptDelivery::Stdin {
            child.stdin.take().map(|mut stdin| {
                thread::spawn(move || {
                    let _ = stdin.write_all(prompt.as_bytes());
                })
            })
        } else {
            None
        };

        let timeout = request.timeout.unwrap_or(self.timeout);
        let status = child
            .wait_timeout(timeout)
            .map_err(|err| PortError::Agent(format!("agent wait failed: {err}")))?;
        let timed_out = status.is_none();
        // Always reap the process group; the agent's background children must
        // not hold our pipes. Read the streams *after* the wait, including on
        // timeout, so a timed-out agent still leaves its output in the trace.
        kill_group(&mut child);
        drop(stdin_writer);
        let (stdout_text, stdout_truncated) = drain_reader(&stdout_rx);
        let (stderr_text, stderr_truncated) = drain_reader(&stderr_rx);
        let (exit_code, signal, success) = match &status {
            Some(status) => {
                let signal = exit_status_signal(status);
                (status.code(), signal, status.success())
            }
            None => (None, None, false),
        };
        let error = if timed_out {
            Some(format!("agent timed out after {}s", timeout.as_secs()))
        } else if !success {
            Some(format!(
                "agent exited with {}: {}",
                exit_code.unwrap_or(-1),
                truncate(stderr_text.trim(), 500)
            ))
        } else {
            None
        };
        Ok(AgentOutcome {
            text: stdout_text.clone(),
            stdout: stdout_text,
            stderr: stderr_text,
            provider: request.provider.as_str().to_owned(),
            argv,
            exit_code,
            timed_out,
            success: success && !timed_out,
            error,
            signal,
            pid: Some(pid),
            env_keys,
            env_hashes,
            stdout_truncated,
            stderr_truncated,
            stdin_sha256,
        })
    }
}

#[cfg(unix)]
fn exit_status_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_status_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn request(provider: AgentProvider) -> AgentRequest {
        AgentRequest {
            run_id: "run".to_owned(),
            step: "step".to_owned(),
            definition: Some("def".to_owned()),
            provider,
            prompt: "prompt".to_owned(),
            system: None,
            model: None,
            command: None,
            prompt_delivery: None,
            env: BTreeMap::new(),
            timeout: None,
            cwd: None,
        }
    }

    #[test]
    fn provider_argv_templates_are_stable() {
        let agent = CommandAgent::default();

        let claude = agent
            .invocation(&request(AgentProvider::Claude))
            .expect("claude");
        assert_eq!(
            claude.argv,
            vec![
                "claude",
                "-p",
                "--output-format",
                "stream-json",
                "--verbose"
            ]
        );
        assert_eq!(claude.delivery, PromptDelivery::Stdin);

        let mut with_model = request(AgentProvider::Claude);
        with_model.model = Some("m".to_owned());
        let claude = agent.invocation(&with_model).expect("claude model");
        assert!(
            claude
                .argv
                .ends_with(&["--model".to_owned(), "m".to_owned()])
        );

        let codex = agent
            .invocation(&request(AgentProvider::Codex))
            .expect("codex");
        assert_eq!(
            codex.argv,
            vec![
                "codex",
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--sandbox",
                "workspace-write",
                ""
            ]
        );
        assert_eq!(codex.delivery, PromptDelivery::Arg);

        let opencode = agent
            .invocation(&request(AgentProvider::Opencode))
            .expect("opencode");
        assert_eq!(
            opencode.argv,
            vec!["opencode", "run", "--format", "json", ""]
        );
        assert_eq!(opencode.delivery, PromptDelivery::Arg);
    }

    #[test]
    fn explicit_command_and_delivery_override_the_provider() {
        let agent = CommandAgent::default();
        let mut request = request(AgentProvider::Opencode);
        request.command = Some(vec!["fake".to_owned(), "--x".to_owned()]);
        request.prompt_delivery = Some(PromptDelivery::Stdin);
        let invocation = agent.invocation(&request).expect("command");
        assert_eq!(invocation.argv, vec!["fake", "--x"]);
        assert_eq!(invocation.delivery, PromptDelivery::Stdin);

        request.prompt_delivery = Some(PromptDelivery::Arg);
        let invocation = agent.invocation(&request).expect("arg");
        assert_eq!(invocation.argv, vec!["fake", "--x", ""]);
        assert_eq!(invocation.delivery, PromptDelivery::Arg);
    }

    #[test]
    fn shell_commands_force_stdin_and_reject_arg_delivery() {
        let agent = CommandAgent::new(Some("fake-agent --flag".to_owned()));
        let invocation = agent
            .invocation(&request(AgentProvider::Command))
            .expect("shell");
        assert_eq!(invocation.delivery, PromptDelivery::Stdin);

        let mut request = request(AgentProvider::Command);
        request.prompt_delivery = Some(PromptDelivery::Arg);
        let error = agent.invocation(&request).expect_err("arg shell");
        assert!(error.to_string().contains("shell-string"));
    }

    #[test]
    fn arg_delivery_rejects_option_like_prompts() {
        let agent = CommandAgent::default();
        let mut request = request(AgentProvider::Codex);
        request.prompt = "--help".to_owned();
        let error = agent.run(&request).expect_err("option prompt");
        assert!(error.to_string().contains("starts with `-`"), "{error}");
    }

    #[test]
    fn missing_command_provider_is_a_configuration_error() {
        let agent = CommandAgent::default();
        let error = agent
            .invocation(&request(AgentProvider::Command))
            .expect_err("missing command");
        assert!(error.to_string().contains("agent step requires"));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod timeout_tests {
    use super::*;

    #[test]
    fn timed_out_agents_still_capture_output() {
        let agent = CommandAgent::default();
        let mut request = request_for_timeout();
        request.command = Some(vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo before-timeout; sleep 5".to_owned(),
        ]);
        request.timeout = Some(Duration::from_millis(150));
        let outcome = agent.run(&request).expect("run");
        assert!(outcome.timed_out);
        assert!(!outcome.success);
        assert!(
            outcome.stdout.contains("before-timeout"),
            "{:?}",
            outcome.stdout
        );
        assert!(outcome.error.expect("error").contains("timed out"));
    }

    #[test]
    fn escaped_descendants_do_not_erase_partial_output() {
        let agent = CommandAgent::default();
        let mut request = request_for_timeout();
        request.command = Some(vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo before-escape; setsid sleep 5 & sleep 5".to_owned(),
        ]);
        request.timeout = Some(Duration::from_millis(150));
        let outcome = agent.run(&request).expect("run");
        assert!(outcome.timed_out);
        assert!(
            outcome.stdout.contains("before-escape"),
            "{:?}",
            outcome.stdout
        );
    }

    fn request_for_timeout() -> AgentRequest {
        AgentRequest {
            run_id: "run".to_owned(),
            step: "step".to_owned(),
            definition: None,
            provider: AgentProvider::Command,
            prompt: "prompt".to_owned(),
            system: None,
            model: None,
            command: None,
            prompt_delivery: None,
            env: BTreeMap::new(),
            timeout: None,
            cwd: None,
        }
    }
}

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use ostinato_application::{CommandExecutor, ExecOutcome, ExecRequest, PortError, PortResult};
use wait_timeout::ChildExt;

const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

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
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = stdout_tx.send(read_limited(stdout));
        });
        thread::spawn(move || {
            let _ = stderr_tx.send(read_limited(stderr));
        });

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
        let stdout = receive_reader(&stdout_rx);
        let stderr = receive_reader(&stderr_rx);
        Ok(ExecOutcome {
            exit_code,
            stdout,
            stderr,
            timed_out,
        })
    }
}

fn read_limited(mut stream: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if buffer.len() < MAX_CAPTURE_BYTES {
                    let remaining = MAX_CAPTURE_BYTES - buffer.len();
                    let take = read.min(remaining);
                    buffer.extend_from_slice(&chunk[..take]);
                    if take < read {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    if truncated {
        buffer.extend_from_slice(b"\n[output truncated]");
    }
    buffer
}

#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{}", child.id())])
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

fn receive_reader(receiver: &Receiver<Vec<u8>>) -> String {
    match receiver.recv_timeout(Duration::from_millis(500)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    }
}

/// Runs an agent command, feeding the prompt on stdin and capturing stdout.
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

    fn resolve_command(&self, override_command: Option<&Vec<String>>) -> PortResult<Vec<String>> {
        override_command
            .cloned()
            .or_else(|| self.command.clone())
            .ok_or_else(|| {
                PortError::Config(
                    "agent step requires `--agent-cmd`, `OSTINATO_AGENT`, or `agent.command`"
                        .to_owned(),
                )
            })
    }
}

impl Default for CommandAgent {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ostinato_application::AgentRunner for CommandAgent {
    fn run(
        &self,
        request: &ostinato_application::AgentRequest,
    ) -> PortResult<ostinato_application::AgentOutcome> {
        let argv = self.resolve_command(request.command.as_ref())?;
        let Some(program) = argv.first() else {
            return Err(PortError::Config("empty agent command".to_owned()));
        };
        let args = &argv[1..];
        let mut process = Command::new(program);
        process
            .args(args)
            .env("OSTINATO_STEP", &request.step)
            .env("OSTINATO_MODEL", request.model.clone().unwrap_or_default())
            .stdin(Stdio::piped())
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
        let mut child = process
            .spawn()
            .map_err(|err| PortError::Agent(format!("cannot spawn agent `{program}`: {err}")))?;

        let prompt = match &request.system {
            Some(system) => format!("SYSTEM:\n{system}\n\nUSER:\n{}", request.prompt),
            None => request.prompt.clone(),
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Agent("agent stdout pipe unavailable".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PortError::Agent("agent stderr pipe unavailable".to_owned()))?;
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = stdout_tx.send(read_limited(stdout));
        });
        thread::spawn(move || {
            let _ = stderr_tx.send(read_limited(stderr));
        });

        // Write stdin from a separate thread: an agent that writes output
        // before reading stdin must not deadlock before the timeout applies.
        let stdin_writer = child.stdin.take().map(|mut stdin| {
            thread::spawn(move || {
                let _ = stdin.write_all(prompt.as_bytes());
            })
        });

        let timeout = request.timeout.unwrap_or(self.timeout);
        let status = child
            .wait_timeout(timeout)
            .map_err(|err| PortError::Agent(format!("agent wait failed: {err}")))?;
        let timed_out = status.is_none();
        // Always reap the process group; the agent's background children must
        // not hold our pipes. The stdin writer is detached and will observe the
        // closed pipe once the group is gone.
        kill_group(&mut child);
        drop(stdin_writer);
        if timed_out {
            return Err(PortError::Agent(format!(
                "agent timed out after {}s",
                timeout.as_secs()
            )));
        }
        let stdout = receive_reader(&stdout_rx);
        let stderr = receive_reader(&stderr_rx);
        let status = status.ok_or_else(|| PortError::Agent("agent status missing".to_owned()))?;
        if !status.success() {
            return Err(PortError::Agent(format!(
                "agent exited with {}: {}",
                status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }
        Ok(ostinato_application::AgentOutcome {
            text: stdout,
            provider: program.clone(),
        })
    }
}

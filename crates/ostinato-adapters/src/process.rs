use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;

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
        let stdout_reader = thread::spawn(move || read_limited(stdout));
        let stderr_reader = thread::spawn(move || read_limited(stderr));

        let status = match request.timeout {
            Some(timeout) => child.wait_timeout(timeout).map_err(|err| {
                PortError::Executor(format!("wait failed for `{}`: {err}", request.program))
            })?,
            None => Some(child.wait().map_err(|err| {
                PortError::Executor(format!("wait failed for `{}`: {err}", request.program))
            })?),
        };
        let timed_out = status.is_none();
        if timed_out {
            kill_group(&mut child);
        }
        let exit_code = status.and_then(|status| status.code()).unwrap_or(-1);
        let stdout = join_reader(stdout_reader);
        let stderr = join_reader(stderr_reader);
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
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_group(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn join_reader(handle: thread::JoinHandle<Vec<u8>>) -> String {
    let bytes = handle.join().unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
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
        let stdout_reader = thread::spawn(move || read_limited(stdout));
        let stderr_reader = thread::spawn(move || read_limited(stderr));

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
        if status.is_none() {
            kill_group(&mut child);
            if let Some(writer) = stdin_writer {
                let _ = writer.join();
            }
            return Err(PortError::Agent(format!(
                "agent timed out after {}s",
                timeout.as_secs()
            )));
        }
        if let Some(writer) = stdin_writer {
            let _ = writer.join();
        }
        let stdout = join_reader(stdout_reader);
        let stderr = join_reader(stderr_reader);
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

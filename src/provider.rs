//! Providers: the things that turn a prompt into a completion.
//!
//! The harness only knows about the `Provider` trait, so adding a new
//! backend (an HTTP API client, a local model, ...) means implementing
//! one method. Two providers ship out of the box:
//!
//!   MockProvider     offline + deterministic. Answers with each task's
//!                    `mock_response` field. Perfect for testing the
//!                    harness itself and for demos without an API key.
//!
//!   CommandProvider  pipes the prompt into any shell command's stdin and
//!                    reads the completion from its stdout. This makes the
//!                    harness work with ANY model you can reach from a
//!                    terminal, with zero HTTP code in this project:
//!                        --provider cmd --cmd "ollama run llama3.2"
//!                        --provider cmd --cmd "python ask_openai.py"

use std::io::Write;
use std::process::{Command, Stdio};

use crate::task::Task;

/// `Send + Sync` is required because the runner calls providers from
/// multiple worker threads at once.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    /// Produce a completion for the task's prompt.
    /// Err(...) means the *provider* failed (process error, etc.),
    /// not that the model answered incorrectly.
    fn complete(&self, task: &Task) -> Result<String, String>;
}

/// Deterministic offline provider used for demos and self-tests.
pub struct MockProvider;

impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn complete(&self, task: &Task) -> Result<String, String> {
        match &task.mock_response {
            Some(r) => Ok(r.clone()),
            // Without a scripted answer the mock politely says so —
            // which will (correctly) fail most graders.
            None => Ok(format!(
                "[mock] no mock_response configured for task `{}`",
                task.id
            )),
        }
    }
}

/// Runs a shell command per task; prompt goes to stdin, answer comes
/// from stdout. Works on Windows (cmd /C) and Unix (sh -c).
pub struct CommandProvider {
    command: String,
}

impl CommandProvider {
    pub fn new(command: String) -> Self {
        CommandProvider { command }
    }
}

impl Provider for CommandProvider {
    fn name(&self) -> &str {
        "cmd"
    }

    fn complete(&self, task: &Task) -> Result<String, String> {
        // Build the platform-appropriate shell invocation.
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&self.command);
            c
        };

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start `{}`: {}", self.command, e))?;

        // Write the prompt to the child's stdin, then drop the handle so
        // the child sees EOF and knows the input is complete.
        {
            let stdin = child
                .stdin
                .take()
                .ok_or("could not open child stdin")?;
            let mut stdin = stdin;
            stdin
                .write_all(task.prompt.as_bytes())
                .map_err(|e| format!("failed writing prompt to child: {}", e))?;
            // stdin dropped here -> EOF for the child.
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("failed waiting for child: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "command exited with {}: {}",
                output.status,
                stderr.trim()
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grader::Grader;

    fn dummy_task(mock: Option<&str>) -> Task {
        Task {
            id: "t".to_string(),
            prompt: "hello".to_string(),
            grader: Grader::Exact,
            expect: "hello".to_string(),
            points: 1,
            mock_response: mock.map(|s| s.to_string()),
        }
    }

    #[test]
    fn mock_returns_scripted_response() {
        let p = MockProvider;
        let t = dummy_task(Some("scripted!"));
        assert_eq!(p.complete(&t).unwrap(), "scripted!");
    }

    #[test]
    fn mock_without_script_mentions_task_id() {
        let p = MockProvider;
        let t = dummy_task(None);
        assert!(p.complete(&t).unwrap().contains("`t`"));
    }

    // An end-to-end CommandProvider test. `cat` on Unix and `findstr ^^`
    // tricks are flaky on Windows, so the echo test is Unix-only.
    #[cfg(unix)]
    #[test]
    fn command_provider_pipes_through_cat() {
        let p = CommandProvider::new("cat".to_string());
        let t = dummy_task(None);
        assert_eq!(p.complete(&t).unwrap(), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn command_provider_reports_failures() {
        let p = CommandProvider::new("exit 3".to_string());
        let t = dummy_task(None);
        assert!(p.complete(&t).is_err());
    }
}

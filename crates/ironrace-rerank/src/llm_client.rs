//! Subprocess client abstraction for LLM rerank.
//!
//! Trait `LlmClient` is the seam: production uses `ClaudeCliClient` (real
//! `claude -p` subprocess), tests use `MockLlmClient` (deterministic fake).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

/// Single-call interface to an LLM. Synchronous, blocking.
pub trait LlmClient: Send + Sync {
    /// Send `prompt` to the LLM, return the assistant's text response.
    /// Errors should NOT leak raw stderr to user-facing layers; sanitize first.
    fn call(&self, prompt: &str) -> Result<String>;
}

/// Real client: shells out to the local `claude` CLI in non-interactive mode.
///
/// Auth uses the user's existing Claude Code subscription (no API key,
/// no per-call cost). Trade-off: subprocess startup overhead ~1-3s per call.
///
/// Invocation:
/// ```text
/// claude --model <model> --output-format json --no-session-persistence \
///        --tools "" --disable-slash-commands -p <prompt>
/// ```
pub struct ClaudeCliClient {
    pub binary: String,
    pub model: String,
    pub timeout: Duration,
}

impl ClaudeCliClient {
    pub fn new(model: impl Into<String>, timeout: Duration) -> Self {
        Self {
            binary: "claude".to_string(),
            model: model.into(),
            timeout,
        }
    }
}

impl LlmClient for ClaudeCliClient {
    fn call(&self, prompt: &str) -> Result<String> {
        let started = Instant::now();
        let mut child = Command::new(&self.binary)
            .arg("--model")
            .arg(&self.model)
            .arg("--output-format")
            .arg("json")
            .arg("--no-session-persistence")
            .arg("--tools")
            .arg("")
            .arg("--disable-slash-commands")
            .arg("-p")
            .arg(prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning {} (is `claude` on PATH?)", self.binary))?;

        // Poll-based wall-clock timeout. Crude but std-only.
        let deadline = started + self.timeout;
        loop {
            match child.try_wait()? {
                Some(_status) => break,
                None => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        bail!("claude CLI timed out after {:?}", self.timeout);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }

        let output = child
            .wait_with_output()
            .context("collecting claude output")?;
        if !output.status.success() {
            // Sanitize stderr — log raw at trace level for debugging, but bubble
            // a generic message up.
            let raw_stderr = String::from_utf8_lossy(&output.stderr);
            tracing::trace!(stderr = %raw_stderr, "claude CLI nonzero exit");
            bail!("claude CLI exited with status {}", output.status);
        }
        String::from_utf8(output.stdout).map_err(|e| anyhow!("claude stdout not UTF-8: {e}"))
    }
}

/// Test-only client. Returns a pre-canned response (or Err) on every `call`.
pub struct MockLlmClient {
    pub response: Result<String>,
}

impl MockLlmClient {
    pub fn ok(response: impl Into<String>) -> Self {
        Self {
            response: Ok(response.into()),
        }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            response: Err(anyhow!(message.into())),
        }
    }
}

impl LlmClient for MockLlmClient {
    fn call(&self, _prompt: &str) -> Result<String> {
        // Mirror the response by cloning the error chain (anyhow::Error isn't Clone).
        match &self.response {
            Ok(s) => Ok(s.clone()),
            Err(e) => bail!("{e}"),
        }
    }
}

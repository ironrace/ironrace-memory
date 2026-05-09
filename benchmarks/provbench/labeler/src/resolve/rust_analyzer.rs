//! rust-analyzer LSP stdio client for symbol resolution.
//!
//! Spawns one `rust-analyzer` process per workspace root, performs the LSP
//! handshake, waits for indexing (with a 30 s wall-clock timeout), and then
//! answers `workspace/symbol` queries.
//!
//! **Transport:** raw `Content-Length: N\r\n\r\n` framing over stdio, using
//! `serde_json::Value` throughout (no `lsp-types` dependency).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::{ResolvedLocation, SymbolResolver};

// ── Types ─────────────────────────────────────────────────────────────────────

/// An LSP client backed by a spawned `rust-analyzer` subprocess.
///
/// A background reader thread owns stdout and forwards complete JSON messages
/// over a channel, which lets `wait_for_indexing` and `send_and_wait` use
/// bounded `recv_timeout` calls instead of blocking reads.
pub struct RustAnalyzer {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
}

// ── LSP framing ──────────────────────────────────────────────────────────────

fn write_message(stdin: &mut impl Write, msg: &Value) -> Result<()> {
    let body = serde_json::to_vec(msg).context("serialize LSP message")?;
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len())
        .context("write LSP Content-Length header")?;
    stdin.write_all(&body).context("write LSP body")?;
    stdin.flush().context("flush LSP stdin")?;
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .context("read LSP header line")?;
        if n == 0 {
            anyhow::bail!("LSP: unexpected EOF reading headers");
        }
        if line == "\r\n" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse().context("parse Content-Length value")?);
        }
    }
    let len = content_length.context("LSP message missing Content-Length header")?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).context("read LSP body bytes")?;
    serde_json::from_slice(&buf).context("parse LSP JSON body")
}

// ── Implementation ────────────────────────────────────────────────────────────

impl RustAnalyzer {
    /// Spawn `rust-analyzer` for the given workspace root, perform the LSP
    /// handshake, and wait for initial indexing to finish (or time out after 30 s).
    pub fn spawn(workspace_root: &Path) -> Result<Self> {
        let workspace_root = workspace_root
            .canonicalize()
            .context("canonicalize workspace root")?;

        let mut child = Command::new("rust-analyzer")
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // suppress RA diagnostic noise
            .spawn()
            .context("spawn rust-analyzer")?;

        let stdin = child.stdin.take().context("take rust-analyzer stdin")?;
        let stdout_raw = child.stdout.take().context("take rust-analyzer stdout")?;

        // Spawn a reader thread that forwards complete JSON messages over a channel.
        // This avoids blocking the calling thread on a raw `read_exact`.
        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout_raw);
            while let Ok(msg) = read_message(&mut reader) {
                if tx.send(msg).is_err() {
                    break; // receiver dropped
                }
            }
            // EOF or parse error — RA process exited; thread exits cleanly.
        });

        let mut ra = Self {
            child,
            stdin,
            rx,
            next_id: 1,
        };

        ra.handshake(&workspace_root)
            .context("LSP handshake with rust-analyzer")?;

        Ok(ra)
    }

    /// Send `initialize` → wait for response → send `initialized` → wait for indexing.
    fn handshake(&mut self, workspace_root: &Path) -> Result<()> {
        let root_uri = path_to_uri(workspace_root);

        let init_params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "provbench-labeler", "version": "0.1" },
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }]
        });

        // initialize request — consume the response (we don't need the server caps).
        self.send_and_wait("initialize", init_params)
            .context("LSP initialize")?;

        // initialized notification (no id, no response expected).
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        });
        write_message(&mut self.stdin, &notif).context("send initialized notification")?;

        // Wait for indexing to complete (or give up after 30 s and try anyway).
        self.wait_for_indexing()
            .context("wait for rust-analyzer indexing")?;

        Ok(())
    }

    /// Drain incoming messages until we see `$/progress` `kind: "end"` after a
    /// prior `begin`/`report`, OR until the message stream goes quiet for
    /// `QUIET_MS` milliseconds (indicating RA finished indexing without sending
    /// progress), OR until the hard 30 s wall-clock deadline elapses.
    ///
    /// For very small workspaces rust-analyzer may never send any `$/progress`
    /// (it finishes before the client even asks); in that case we fall through
    /// after the quiet period and proceed directly to `workspace/symbol`.
    fn wait_for_indexing(&mut self) -> Result<()> {
        // After this many ms without any new message, assume RA is idle.
        const QUIET_MS: u64 = 1_000;
        let hard_deadline = Instant::now() + Duration::from_secs(30);
        let mut seen_begin = false;

        loop {
            let now = Instant::now();
            if now >= hard_deadline {
                tracing::warn!(
                    "rust-analyzer indexing did not signal $/progress end within 30 s; proceeding"
                );
                return Ok(());
            }

            // Wait at most QUIET_MS or the remaining hard deadline, whichever is shorter.
            let hard_remaining = hard_deadline.saturating_duration_since(now);
            let wait = hard_remaining.min(Duration::from_millis(QUIET_MS));

            let msg = match self.rx.recv_timeout(wait) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if seen_begin {
                        // We started getting progress but it stopped — still wait.
                        continue;
                    }
                    // No messages at all for QUIET_MS — RA finished with no progress.
                    tracing::debug!("rust-analyzer sent no $/progress; assuming indexing complete");
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("rust-analyzer stdout closed during indexing wait");
                }
            };

            if let Some(method) = msg.get("method").and_then(Value::as_str) {
                if method == "$/progress" {
                    if let Some(kind) = msg
                        .get("params")
                        .and_then(|p| p.get("value"))
                        .and_then(|v| v.get("kind"))
                        .and_then(Value::as_str)
                    {
                        match kind {
                            "begin" | "report" => {
                                seen_begin = true;
                            }
                            "end" if seen_begin => {
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Send a request and return the `result` field of the matching response,
    /// skipping intervening notifications. Waits up to 60 s for the response.
    fn send_and_wait(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        write_message(&mut self.stdin, &req)
            .with_context(|| format!("write LSP request `{method}`"))?;

        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .context("timeout waiting for LSP response")?;

            let msg = match self.rx.recv_timeout(remaining) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    anyhow::bail!("timeout waiting for LSP response to `{method}`");
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("rust-analyzer stdout closed while waiting for `{method}`");
                }
            };

            // Check whether this message has our id.
            if msg.get("id").map(|v| v == &json!(id)).unwrap_or(false) {
                if let Some(err) = msg.get("error") {
                    anyhow::bail!("LSP error from `{method}`: {err}");
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }

            // Notification — log progress, drop the rest.
            if let Some(m) = msg.get("method").and_then(Value::as_str) {
                if m == "$/progress" {
                    if let Some(kind) = msg
                        .get("params")
                        .and_then(|p| p.get("value"))
                        .and_then(|v| v.get("kind"))
                        .and_then(Value::as_str)
                    {
                        tracing::debug!("RA progress kind={kind} during `{method}`");
                    }
                }
            }
        }
    }

    /// Best-effort shutdown: send `shutdown` request + `exit` notification,
    /// then kill the child if it doesn't exit within 500 ms.
    ///
    /// This must NOT panic — all errors are logged via `tracing::warn`.
    fn shutdown(&mut self) {
        let id = self.next_id;
        self.next_id += 1;

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown",
            "params": Value::Null
        });

        if let Err(e) = write_message(&mut self.stdin, &req) {
            tracing::warn!("rust-analyzer shutdown write error: {e}");
        } else {
            // Wait briefly for the shutdown response (non-blocking via channel).
            let deadline = Instant::now() + Duration::from_secs(3);
            'outer: while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                match self
                    .rx
                    .recv_timeout(remaining.min(Duration::from_millis(200)))
                {
                    Ok(msg) if msg.get("id").map(|v| v == &json!(id)).unwrap_or(false) => {
                        break 'outer;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }

        // exit notification — best-effort.
        let notif = json!({"jsonrpc": "2.0", "method": "exit", "params": Value::Null});
        if let Err(e) = write_message(&mut self.stdin, &notif) {
            tracing::warn!("rust-analyzer exit notification error: {e}");
        }

        // Wait up to 500 ms for the child to exit, then kill.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    tracing::warn!("rust-analyzer wait error: {e}");
                    break;
                }
            }
        }
        if let Err(e) = self.child.kill() {
            tracing::warn!("rust-analyzer kill error: {e}");
        }
    }
}

impl Drop for RustAnalyzer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl SymbolResolver for RustAnalyzer {
    fn resolve(&mut self, qualified_name: &str) -> Result<Option<ResolvedLocation>> {
        // Query using the last path segment (workspace/symbol uses a simple name filter).
        let last_segment = qualified_name.split("::").last().unwrap_or(qualified_name);

        let params = json!({ "query": last_segment });
        let result = self
            .send_and_wait("workspace/symbol", params)
            .context("workspace/symbol request")?;

        if result.is_null() {
            return Ok(None);
        }

        let symbols = result
            .as_array()
            .context("workspace/symbol result is not an array")?;

        if symbols.is_empty() {
            return Ok(None);
        }

        // 1. Prefer an exact full-qualified match (containerName::name or bare name).
        // 2. Fall back to the first symbol whose `name` equals the last segment.
        let best = symbols
            .iter()
            .find(|sym| {
                let name = sym.get("name").and_then(Value::as_str).unwrap_or("");
                let container = sym
                    .get("containerName")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let fqn = if container.is_empty() {
                    name.to_string()
                } else {
                    format!("{container}::{name}")
                };
                fqn == qualified_name || name == qualified_name
            })
            .or_else(|| {
                symbols.iter().find(|sym| {
                    sym.get("name").and_then(Value::as_str).unwrap_or("") == last_segment
                })
            });

        let sym = match best {
            Some(s) => s,
            None => return Ok(None),
        };

        // Extract the file URI and line from the symbol location.
        let loc = sym.get("location").context("symbol missing `location`")?;
        let uri = loc
            .get("uri")
            .and_then(Value::as_str)
            .context("symbol location missing `uri`")?;
        let range = loc
            .get("range")
            .context("symbol location missing `range`")?;
        let line_0based = range
            .get("start")
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .context("symbol range missing `start.line`")? as u32;

        let file = uri_to_path(uri).context("parse symbol URI")?;

        Ok(Some(ResolvedLocation {
            file,
            line: line_0based + 1, // LSP is 0-based; ResolvedLocation is 1-based
        }))
    }
}

// ── URI helpers ───────────────────────────────────────────────────────────────

fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("file://{s}")
}

fn uri_to_path(uri: &str) -> Result<std::path::PathBuf> {
    let path_str = uri
        .strip_prefix("file://")
        .with_context(|| format!("URI does not start with file://: {uri}"))?;
    Ok(std::path::PathBuf::from(percent_decode(path_str)))
}

/// Minimal percent-decoding for file URIs (handles `%20`, `%3A`, etc.).
fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

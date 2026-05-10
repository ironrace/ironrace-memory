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

const MAX_LSP_MSG: usize = 64 * 1024 * 1024;

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
    if len > MAX_LSP_MSG {
        anyhow::bail!("LSP Content-Length {len} exceeds cap {MAX_LSP_MSG}");
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).context("read LSP body bytes")?;
    serde_json::from_slice(&buf).context("parse LSP JSON body")
}

// ── Indexing state machine (extracted for unit testing without spawning RA) ──

/// After this many ms without any new LSP message, assume RA is idle.
const INDEXING_QUIET_MS: u64 = 1_000;

/// Pure(-ish) indexing wait state machine driven by an injectable message
/// receiver. Mirrors the live `recv_timeout` API so production code can pass
/// `|d| self.rx.recv_timeout(d)` and tests can pass a synthetic queue.
///
/// Behavior:
/// - Returns `Ok(())` once a `$/progress` `end` is seen following a prior
///   `begin`/`report`.
/// - Returns `Ok(())` if no messages arrive for `INDEXING_QUIET_MS` AND no
///   `begin` has yet been observed (small workspaces that finish indexing
///   instantly never emit progress).
/// - Returns `Err(_)` if a `begin` was observed but the hard deadline elapses
///   before a matching `end` (fail-closed: replay must not silently proceed
///   with possibly incomplete symbol data). The error message includes the
///   workspace root.
/// - Returns `Err(_)` if the message channel disconnects mid-wait.
fn run_indexing_state_machine<F>(
    workspace_root: &Path,
    hard_deadline: Instant,
    mut recv_timeout: F,
) -> Result<()>
where
    F: FnMut(Duration) -> std::result::Result<Value, mpsc::RecvTimeoutError>,
{
    let mut progress_started = false;

    loop {
        let now = Instant::now();
        if now >= hard_deadline {
            if progress_started {
                anyhow::bail!(
                    "rust-analyzer indexing timed out at {}",
                    workspace_root.display()
                );
            }
            // No begin observed and deadline elapsed — treat as quiet success
            // (extremely small workspace where RA never spoke at all).
            tracing::debug!(
                "rust-analyzer indexing deadline reached with no $/progress; assuming complete"
            );
            return Ok(());
        }

        // Wait at most INDEXING_QUIET_MS or the remaining hard deadline,
        // whichever is shorter.
        let hard_remaining = hard_deadline.saturating_duration_since(now);
        let wait = hard_remaining.min(Duration::from_millis(INDEXING_QUIET_MS));

        let msg = match recv_timeout(wait) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if progress_started {
                    // We started getting progress but it stopped — keep waiting
                    // for an explicit end up to the hard deadline.
                    continue;
                }
                // No messages at all for INDEXING_QUIET_MS — RA finished with
                // no progress notifications.
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
                            progress_started = true;
                        }
                        "end" if progress_started => {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
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

        // Wait for indexing to complete; fail closed if begin observed but
        // never ended before the 30 s hard deadline.
        self.wait_for_indexing(workspace_root)
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
    ///
    /// **Fail-closed semantics:** if RA emitted at least one `$/progress`
    /// `begin`/`report` but never reached `end` before the hard 30 s deadline,
    /// return an error rather than silently proceeding with possibly
    /// incomplete symbol resolution.
    fn wait_for_indexing(&mut self, workspace_root: &Path) -> Result<()> {
        let hard_deadline = Instant::now() + Duration::from_secs(30);
        run_indexing_state_machine(workspace_root, hard_deadline, |timeout| {
            self.rx.recv_timeout(timeout)
        })
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

pub(crate) fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("file://{s}")
}

pub(crate) fn uri_to_path(uri: &str) -> Result<std::path::PathBuf> {
    let path_str = uri
        .strip_prefix("file://")
        .with_context(|| format!("URI does not start with file://: {uri}"))?;
    Ok(std::path::PathBuf::from(percent_decode(path_str)?))
}

/// Minimal percent-decoding for file URIs (handles `%20`, `%3A`, multi-byte
/// UTF-8 sequences like `%E2%94%80`, etc.).
///
/// Decoded bytes are accumulated into a `Vec<u8>` and validated as UTF-8 at
/// the end so that multi-byte sequences (e.g. `─` = `E2 94 80`) round-trip
/// correctly. Per-byte `as char` conversion would corrupt these by emitting
/// one `char` per raw byte (each in U+0080..=U+00FF) instead of decoding the
/// full code point.
pub(crate) fn percent_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).with_context(|| format!("invalid UTF-8 in percent-decoded URI: {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_message_rejects_content_length_above_cap() {
        let input = format!("Content-Length: {}\r\n\r\n", MAX_LSP_MSG + 1);
        let err = read_message(&mut Cursor::new(input)).unwrap_err();
        assert!(
            err.to_string().contains("exceeds cap"),
            "unexpected error: {err}"
        );
    }

    // ── percent-decoding (UTF-8 safety) ──────────────────────────────────────

    #[test]
    fn percent_decode_handles_multibyte_utf8_box_drawing() {
        // `─` (U+2500) is `E2 94 80` in UTF-8; two of them in a directory name.
        let decoded = percent_decode("/tmp/%E2%94%80%E2%94%80/foo.rs").unwrap();
        assert_eq!(decoded, "/tmp/──/foo.rs");
    }

    #[test]
    fn percent_decode_handles_multibyte_utf8_latin1_e_acute() {
        // `é` (U+00E9) is `C3 A9` in UTF-8 (2 bytes).
        let decoded = percent_decode("/tmp/caf%C3%A9/menu.rs").unwrap();
        assert_eq!(decoded, "/tmp/café/menu.rs");
    }

    #[test]
    fn percent_decode_preserves_space_encoding() {
        let decoded = percent_decode("/tmp/foo%20bar.rs").unwrap();
        assert_eq!(decoded, "/tmp/foo bar.rs");
    }

    #[test]
    fn percent_decode_preserves_unencoded_bytes() {
        let decoded = percent_decode("/plain/path/no_escapes.rs").unwrap();
        assert_eq!(decoded, "/plain/path/no_escapes.rs");
    }

    #[test]
    fn percent_decode_rejects_invalid_utf8_sequence() {
        // `%C3` alone (without a continuation byte) is an invalid UTF-8 prefix.
        let err = percent_decode("/tmp/bad%C3.rs").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid UTF-8 in percent-decoded URI"),
            "unexpected error: {msg}"
        );
    }

    // ── uri_to_path ──────────────────────────────────────────────────────────

    #[test]
    fn uri_to_path_decodes_multibyte_utf8() {
        let path = uri_to_path("file:///tmp/%E2%94%80%E2%94%80/foo.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("/tmp/──/foo.rs"));
    }

    #[test]
    fn uri_to_path_decodes_space() {
        let path = uri_to_path("file:///tmp/foo%20bar.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("/tmp/foo bar.rs"));
    }

    #[test]
    fn uri_to_path_rejects_non_file_scheme() {
        let err = uri_to_path("https://example.com/foo.rs").unwrap_err();
        assert!(format!("{err:#}").contains("URI does not start with file://"));
    }

    // ── wait_for_indexing state machine ──────────────────────────────────────

    /// Build a queue-backed `recv_timeout` shim. Each call pops the next
    /// `Result` from the front of the queue. When the queue is empty we
    /// simulate `Timeout` so the quiet-period branch can trigger.
    fn queue_recv(
        queue: std::rc::Rc<std::cell::RefCell<std::collections::VecDeque<Value>>>,
    ) -> impl FnMut(Duration) -> std::result::Result<Value, mpsc::RecvTimeoutError> {
        move |_d: Duration| match queue.borrow_mut().pop_front() {
            Some(v) => Ok(v),
            None => Err(mpsc::RecvTimeoutError::Timeout),
        }
    }

    fn progress(kind: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": "ra/indexing", "value": { "kind": kind } }
        })
    }

    #[test]
    fn wait_for_indexing_fails_closed_when_begin_without_end_before_deadline() {
        // Begin observed, but no end. The loop must process `begin` (setting
        // progress_started = true) BEFORE the deadline check fires, otherwise
        // we'd take the quiet-period branch. We achieve that by:
        //   1. Letting `recv_timeout` return `begin` on the first call.
        //   2. Returning `Timeout` thereafter, which falls into the
        //      `progress_started ? continue : Ok(())` branch.
        //   3. Setting a near-future deadline (500 ms) so the next loop turn
        //      trips the `now >= hard_deadline` check with progress_started=true.
        //
        // We use 500 ms deadline + 800 ms sleep (300 ms slack) to absorb CI
        // scheduler jitter on shared GitHub Actions runners (15-50 ms typical,
        // occasionally higher) without introducing flakes.
        let workspace_root = std::path::PathBuf::from("/tmp/fake-ws-root-deadline");
        let hard_deadline = Instant::now() + Duration::from_millis(500);

        let mut delivered_begin = false;
        let recv = move |_d: Duration| -> std::result::Result<Value, mpsc::RecvTimeoutError> {
            if !delivered_begin {
                delivered_begin = true;
                return Ok(progress("begin"));
            }
            // Sleep well past the deadline so the next loop turn observes
            // `now >= hard_deadline` while progress_started is true.
            std::thread::sleep(Duration::from_millis(800));
            Err(mpsc::RecvTimeoutError::Timeout)
        };

        let err = run_indexing_state_machine(&workspace_root, hard_deadline, recv).unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("rust-analyzer indexing timed out"),
            "expected timeout error, got: {msg}"
        );
        assert!(
            msg.contains("/tmp/fake-ws-root-deadline"),
            "expected workspace root in error, got: {msg}"
        );
    }

    #[test]
    fn wait_for_indexing_returns_ok_when_no_progress_messages_within_quiet_period() {
        // Empty queue → recv_timeout always returns Timeout → progress_started
        // never set → quiet-period success path returns Ok(()).
        let queue: std::rc::Rc<std::cell::RefCell<std::collections::VecDeque<Value>>> =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));

        let workspace_root = std::path::PathBuf::from("/tmp/fake-ws-root-quiet");
        let hard_deadline = Instant::now() + Duration::from_secs(30);

        run_indexing_state_machine(&workspace_root, hard_deadline, queue_recv(queue))
            .expect("zero-progress workspace must succeed via quiet-period path");
    }

    #[test]
    fn wait_for_indexing_returns_ok_when_begin_followed_by_end() {
        // Sanity: the happy path (begin → end) still completes successfully.
        let queue: std::rc::Rc<std::cell::RefCell<std::collections::VecDeque<Value>>> =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::from(
                vec![progress("begin"), progress("report"), progress("end")],
            )));

        let workspace_root = std::path::PathBuf::from("/tmp/fake-ws-root-happy");
        let hard_deadline = Instant::now() + Duration::from_secs(30);

        run_indexing_state_machine(&workspace_root, hard_deadline, queue_recv(queue))
            .expect("begin→end sequence must succeed");
    }
}

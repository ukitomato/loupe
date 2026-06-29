// mcp — Model Context Protocol server over stdio (JSON-RPC 2.0, newline-delimited).
//
// Implemented by hand rather than via the `rmcp` SDK: the MCP stdio wire format is a small, stable
// subset of JSON-RPC (initialize / tools/list / tools/call / ping), the rest of this crate is
// synchronous (rmcp would pull in a full tokio runtime), and the existing `serve` daemon already
// speaks line-delimited JSON over stdio — so a hand-rolled loop keeps the binary lean and lets us
// verify the protocol offline. Swapping in rmcp later is possible without touching the index core.
//
// stdout carries protocol messages ONLY; all diagnostics go to stderr.
//
// Tools:
//   search_code(query, max_results?)    trigram + substring search
//   search_regex(pattern, max_results?) trigram + regex search
//   build_index(force?)                 full (re)build from settings.json roots
//   sync_index()                        incremental catch-up
//   index_status()                      stats as JSON text

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Instant;

use crate::index::searcher::Hit;
use crate::index::{builder, open_state, searcher, State};
use crate::store;
use crate::watcher::start_watcher;

const DEFAULT_PROTOCOL: &str = "2024-11-05";
const DEFAULT_MAX_RESULTS: usize = 100;
const BUILD_MAX_ATTEMPTS: u32 = 6;

pub fn run(index_dir: Option<&str>) -> Result<()> {
    let mut server = McpServer {
        dir: store::resolve_index_dir(index_dir),
        state: None,
        watcher_stop: None,
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                write_msg(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}),
                );
                continue;
            }
        };
        if let Some(resp) = server.handle(&msg) {
            write_msg(&mut out, &resp);
        }
    }
    Ok(())
}

fn write_msg(out: &mut std::io::Stdout, v: &Value) {
    let mut lock = out.lock();
    let _ = writeln!(lock, "{v}");
    let _ = lock.flush();
}

struct McpServer {
    dir: std::path::PathBuf,
    state: Option<Arc<State>>,
    watcher_stop: Option<mpsc::SyncSender<()>>,
}

impl McpServer {
    fn state(&mut self) -> Result<Arc<State>> {
        if self.state.is_none() {
            let s = open_state(&store::tantivy_dir(&self.dir))?;
            // Keep the long-lived server fresh without the caller asking: set the configured roots
            // and start the file watcher so edits made during the session are reflected in searches.
            if let Ok(roots) = store::resolved_roots(&self.dir) {
                if !roots.is_empty() {
                    s.set_roots(&roots);
                    self.replace_watcher(&s);
                }
            }
            self.state = Some(s);
        }
        Ok(self.state.clone().unwrap())
    }

    /// Stop the current watcher (if any) and start a fresh one for the given state.
    fn replace_watcher(&mut self, state: &Arc<State>) {
        if let Some(stop) = self.watcher_stop.take() {
            let _ = stop.send(());
        }
        self.watcher_stop = start_watcher(state.clone()).ok();
    }

    /// Returns Some(response) for requests; None for notifications.
    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let is_notification = id.is_none();
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        if is_notification {
            return None; // initialized / cancelled / progress — nothing to reply
        }

        let result: std::result::Result<Value, (i64, String)> = match method {
            "initialize" => Ok(self.initialize(&params)),
            "tools/list" => Ok(self.tools_list()),
            "tools/call" => self.tools_call(&params),
            "ping" => Ok(json!({})),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        let id = id.unwrap_or(Value::Null);
        Some(match result {
            Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err((code, message)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
            }
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let pv = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_PROTOCOL)
            .to_string();
        json!({
            "protocolVersion": pv,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "indexify", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    fn tools_list(&self) -> Value {
        json!({ "tools": [
            {
                "name": "search_code",
                "description": "Full-text substring search across the indexed code (UTF-8 and Shift_JIS folders). Returns matching path:line: text. Needs a >=2 character query (a 2-char query, e.g. a Japanese word like 契約, is supported).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Substring to search for (case-insensitive, >=2 chars)." },
                        "max_results": { "type": "integer", "description": "Max results (default 100)." }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "search_regex",
                "description": "Regular-expression search across the indexed code. The pattern must contain a literal run of >=2 chars (ASCII or CJK) so the index can pre-filter candidates.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Regex pattern (case-insensitive). Must contain a >=2 char literal run." },
                        "max_results": { "type": "integer", "description": "Max results (default 100)." }
                    },
                    "required": ["pattern"]
                }
            },
            {
                "name": "build_index",
                "description": "Build (or rebuild) the index from the roots configured in settings.json (whole workspace if none are configured). Can take a long time on a large repo.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "force": { "type": "boolean", "description": "Discard the existing index and rebuild from scratch." }
                    }
                }
            },
            {
                "name": "sync_index",
                "description": "Incrementally update the index: reindex changed/new files and drop deleted ones.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "index_status",
                "description": "Report index statistics: file count, configured roots, last build/sync time, on-disk size.",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]})
    }

    fn tools_call(&mut self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or((-32602, "missing tool name".to_string()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let res = match name {
            "search_code" => self.tool_search(&args, false),
            "search_regex" => self.tool_search(&args, true),
            "build_index" => self.tool_build(&args),
            "sync_index" => self.tool_sync(),
            "index_status" => self.tool_status(),
            _ => return Err((-32602, format!("unknown tool: {name}"))),
        };
        Ok(res)
    }

    fn tool_search(&mut self, args: &Value, regex: bool) -> Value {
        if !store::index_built(&self.dir) {
            return text_result(
                format!(
                    "index_not_found: no index at {}. Run the build_index tool (or `indexify build`) first.",
                    self.dir.display()
                ),
                true,
            );
        }
        let key = if regex { "pattern" } else { "query" };
        let query = args.get(key).and_then(|v| v.as_str()).unwrap_or("");
        if query.is_empty() {
            return text_result(format!("missing `{key}`"), true);
        }
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);
        let state = match self.state() {
            Ok(s) => s,
            Err(e) => return text_result(format!("error opening index: {e}"), true),
        };
        match searcher::search(&state, query, regex, max, false) {
            Ok(outcome) => text_result(
                self.format_hits(&outcome.hits, outcome.candidates_truncated),
                false,
            ),
            Err(e) => text_result(format!("search error: {e}"), true),
        }
    }

    fn tool_build(&mut self, args: &Value) -> Value {
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let roots = match store::resolved_roots_or_default(&self.dir) {
            Ok(r) => r,
            Err(e) => return text_result(e.to_string(), true),
        };
        store::ensure_gitignore(&self.dir).ok();
        let tdir = store::tantivy_dir(&self.dir);
        if force && tdir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&tdir) {
                return text_result(format!("could not clear index: {e}"), true);
            }
            self.state = None;
        }
        let state = match self.state() {
            Ok(s) => s,
            Err(e) => return text_result(format!("error opening index: {e}"), true),
        };
        state.set_roots(&roots);
        let t0 = Instant::now();
        let mut total = 0u64;
        for (abs, enc) in &roots {
            let rs = abs.to_string_lossy().into_owned();
            match build_retry(&state, &rs, enc) {
                Ok(n) => total += n,
                Err(e) => return text_result(format!("build error on {rs}: {e}"), true),
            }
        }
        let secs = t0.elapsed().as_secs_f64();
        self.update_meta(&state, true);
        self.replace_watcher(&state);
        text_result(format!("built {total} files in {secs:.1}s"), false)
    }

    fn tool_sync(&mut self) -> Value {
        if !store::index_built(&self.dir) {
            return text_result(
                format!(
                    "index_not_found: no index at {}. Run build_index first.",
                    self.dir.display()
                ),
                true,
            );
        }
        let roots = match store::resolved_roots(&self.dir) {
            Ok(r) => r,
            Err(e) => return text_result(e.to_string(), true),
        };
        let state = match self.state() {
            Ok(s) => s,
            Err(e) => return text_result(format!("error opening index: {e}"), true),
        };
        state.set_roots(&roots);
        let t0 = Instant::now();
        let mut result = builder::sync_all(&state, |_| {});
        let mut attempt = 1;
        while result.is_err() && attempt < BUILD_MAX_ATTEMPTS {
            attempt += 1;
            let _ = builder::recreate_writer(&state);
            result = builder::sync_all(&state, |_| {});
        }
        match result {
            Ok(stats) => {
                let secs = t0.elapsed().as_secs_f64();
                self.update_meta(&state, false);
                self.replace_watcher(&state);
                text_result(
                    format!(
                        "synced: {} updated, {} removed in {secs:.1}s",
                        stats.updated, stats.removed
                    ),
                    false,
                )
            }
            Err(e) => text_result(format!("sync error: {e}"), true),
        }
    }

    fn tool_status(&mut self) -> Value {
        let built = store::index_built(&self.dir);
        let cfg = store::load_config(&self.dir).unwrap_or_default();
        let meta = store::load_meta(&self.dir);
        let size = store::index_size_bytes(&self.dir);
        let file_count = if built {
            self.state()
                .map(|s| s.num_docs())
                .unwrap_or(meta.file_count)
        } else {
            meta.file_count
        };
        let roots: Vec<_> = cfg
            .roots
            .iter()
            .map(|r| json!({ "path": r.path, "encoding": r.encoding }))
            .collect();
        let status = json!({
            "built": built,
            "index_dir": self.dir.display().to_string(),
            "file_count": file_count,
            "size_bytes": size,
            "last_build": meta.last_build,
            "last_sync": meta.last_sync,
            "roots": roots,
        });
        text_result(
            serde_json::to_string_pretty(&status).unwrap_or_else(|_| status.to_string()),
            false,
        )
    }

    fn update_meta(&self, state: &State, built: bool) {
        let mut meta = store::load_meta(&self.dir);
        let now = store::now_rfc3339();
        if built {
            meta.last_build = Some(now);
        } else {
            meta.last_sync = Some(now);
        }
        meta.file_count = state.num_docs();
        let _ = store::save_meta(&self.dir, &meta);
    }

    fn format_hits(&self, hits: &[Hit], truncated: bool) -> String {
        if hits.is_empty() {
            return "No matches.".to_string();
        }
        let root = store::workspace_root(&self.dir);
        let mut s = String::new();
        for h in hits {
            let p = Path::new(&h.file)
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| h.file.clone());
            s.push_str(&format!("{}:{}: {}\n", p, h.line, h.text));
        }
        s.push_str(&format!("\n{} results", hits.len()));
        if truncated {
            s.push_str(
                "\n(note: candidate limit reached — results may be incomplete; narrow the query for full coverage)",
            );
        }
        s
    }
}

fn build_retry(state: &State, root: &str, enc: &str) -> Result<u64> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match builder::build_root(state, root, enc, |_| {}) {
            Ok(n) => return Ok(n),
            Err(_) if attempt < BUILD_MAX_ATTEMPTS => {
                let _ = builder::recreate_writer(state);
            }
            Err(e) => return Err(e),
        }
    }
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

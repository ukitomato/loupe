// serve — long-running NDJSON daemon used by the VSCode extension.
//
// Roots are NOT taken from the client; they come from settings.json (the shared source of truth),
// so the extension, the CLI, and the MCP server all index exactly the same folders. The client only
// triggers actions. If settings.json has no roots yet, build/sync bootstrap it to the whole
// workspace (see store::resolved_roots_or_default).
//
// Protocol: one JSON object per line on stdin; JSON objects per line on stdout.
//   ->  {"id":1,"cmd":"build"}
//   <-  {"id":1,"type":"progress","indexed":20000} ... {"id":1,"type":"built","files":N,"ms":N}
//   ->  {"id":2,"cmd":"search","query":"foo","regex":false,"max":300}
//   <-  {"id":2,"type":"match","file":"..","line":12,"text":".."} ... {"id":2,"type":"done","hits":N}
//   ->  {"id":3,"cmd":"sync"}     <- {"id":3,"type":"synced","updated":N,"removed":N}
//   ->  {"id":4,"cmd":"watch"}    <- {"id":4,"type":"watching"}
//   ->  {"id":5,"cmd":"stop"}     <- {"id":5,"type":"stopped"}
// After a build/sync the daemon watches the roots and updates the index incrementally.

use anyhow::Result;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Instant;

use crate::index::{builder, open_state, searcher, State};
use crate::store;
use crate::watcher::start_watcher;

const MAX_ATTEMPTS: u32 = 6;

pub fn run(index_dir: Option<&str>) -> Result<()> {
    let dir = store::resolve_index_dir(index_dir);
    store::ensure_gitignore(&dir).ok();
    let state = open_state(&store::tantivy_dir(&dir))?;
    emit(serde_json::json!({ "type": "ready" }));

    // Stop sender for the active file watcher. Replaced on each build/sync/watch command;
    // the old watcher thread receives the signal and exits before the new one starts.
    let mut watcher_stop: Option<mpsc::SyncSender<()>> = None;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                emit(serde_json::json!({ "type": "error", "message": format!("bad json: {e}") }));
                continue;
            }
        };
        match handle_cmd(&state, &dir, &req, &mut watcher_stop) {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => emit(serde_json::json!({ "type": "error", "message": e.to_string() })),
        }
    }
    Ok(())
}

fn emit(v: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn update_meta(dir: &Path, state: &State, built: bool) {
    let mut meta = store::load_meta(dir);
    let now = store::now_rfc3339();
    if built {
        meta.last_build = Some(now);
    } else {
        meta.last_sync = Some(now);
    }
    meta.file_count = state.num_docs();
    let _ = store::save_meta(dir, &meta);
}

/// Stop the current watcher (if any) and start a new one, storing the stop handle.
fn replace_watcher(state: &Arc<State>, watcher_stop: &mut Option<mpsc::SyncSender<()>>) {
    if let Some(stop) = watcher_stop.take() {
        let _ = stop.send(());
    }
    *watcher_stop = start_watcher(state.clone()).ok();
}

fn handle_cmd(
    state: &Arc<State>,
    dir: &Path,
    req: &serde_json::Value,
    watcher_stop: &mut Option<mpsc::SyncSender<()>>,
) -> Result<bool> {
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    match req.get("cmd").and_then(|v| v.as_str()) {
        Some("build") => {
            let roots = store::resolved_roots_or_default(dir)?;
            state.set_roots(&roots);
            let t0 = Instant::now();
            let mut total = 0u64;
            for (abs, enc) in &roots {
                let root_str = abs.to_string_lossy().into_owned();
                let id2 = id.clone();
                let mut attempt = 0;
                loop {
                    attempt += 1;
                    let prog = |n: u64| {
                        emit(serde_json::json!({ "id": id2, "type": "progress", "indexed": n }))
                    };
                    match builder::build_root(state, &root_str, enc, prog) {
                        Ok(n) => {
                            total += n;
                            break;
                        }
                        Err(e) if attempt < MAX_ATTEMPTS => {
                            emit(
                                serde_json::json!({ "id": id, "type": "progress", "message": format!("retrying build (attempt {attempt}) after error: {e}") }),
                            );
                            builder::recreate_writer(state, &store::tantivy_dir(dir))?;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            let ms = t0.elapsed().as_millis() as u64;
            update_meta(dir, state, true);
            emit(serde_json::json!({ "id": id, "type": "built", "files": total, "ms": ms }));
            replace_watcher(state, watcher_stop);
            Ok(false)
        }
        Some("sync") => {
            let roots = store::resolved_roots_or_default(dir)?;
            state.set_roots(&roots);
            let t0 = Instant::now();
            let id2 = id.clone();
            let prog =
                |n: u64| emit(serde_json::json!({ "id": id2, "type": "progress", "indexed": n }));
            let mut result = builder::sync_all(state, prog);
            let mut attempt = 1;
            while result.is_err() && attempt < MAX_ATTEMPTS {
                attempt += 1;
                result = builder::recreate_writer(state, &store::tantivy_dir(dir))
                    .and_then(|_| builder::sync_all(state, prog));
            }
            let stats = result?;
            let ms = t0.elapsed().as_millis() as u64;
            update_meta(dir, state, false);
            emit(
                serde_json::json!({ "id": id, "type": "synced", "updated": stats.updated, "removed": stats.removed, "ms": ms }),
            );
            replace_watcher(state, watcher_stop);
            Ok(false)
        }
        Some("search") => {
            let q = req.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let regex = req.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
            let max = req.get("max").and_then(|v| v.as_u64()).unwrap_or(300) as usize;
            let case_sensitive = req
                .get("caseSensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let outcome = searcher::search(state, q, regex, max, case_sensitive)?;
            let n = outcome.hits.len();
            for h in outcome.hits {
                emit(
                    serde_json::json!({ "id": id, "type": "match", "file": h.file, "line": h.line, "text": h.text }),
                );
            }
            emit(serde_json::json!({ "id": id, "type": "done", "hits": n, "truncated": outcome.candidates_truncated }));
            Ok(false)
        }
        Some("watch") => {
            if let Ok(roots) = store::resolved_roots(dir) {
                state.set_roots(&roots);
            }
            replace_watcher(state, watcher_stop);
            emit(serde_json::json!({ "id": id, "type": "watching" }));
            Ok(false)
        }
        Some("stop") => {
            emit(serde_json::json!({ "id": id, "type": "stopped" }));
            Ok(true)
        }
        other => {
            emit(
                serde_json::json!({ "id": id, "type": "error", "message": format!("unknown cmd: {other:?}") }),
            );
            Ok(false)
        }
    }
}

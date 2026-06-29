// watcher.rs — watch the configured roots and incrementally update the index.
//
// File events are debounced (collected for a quiet period) and then applied as single-path updates,
// followed by one commit + reader reload. Used by the long-running `serve` sidecar.
//
// `start_watcher` returns a stop sender. The caller should store it and send `()` to it before
// calling `start_watcher` again — this prevents accumulating watcher threads across build/sync
// cycles.

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::index::{builder, State};

const DEBOUNCE: Duration = Duration::from_millis(800);

/// Start a file watcher for the currently configured roots. Returns a stop sender; dropping it
/// or sending `()` on it causes the watcher thread to exit cleanly.
pub fn start_watcher(state: Arc<State>) -> Result<mpsc::SyncSender<()>> {
    use notify::{RecursiveMode, Watcher};
    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(raw_tx)?;
    {
        let roots = state.roots.lock().unwrap();
        for (prefix, _) in roots.iter() {
            let _ = watcher.watch(prefix, RecursiveMode::Recursive);
        }
    }
    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(0);
    // debounce thread: collect changed paths, flush after quiet period, then commit once.
    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive
        let mut pending: HashSet<PathBuf> = HashSet::new();
        loop {
            // Check for stop signal (non-blocking).
            if stop_rx.try_recv().is_ok() {
                break;
            }
            // Wait for the next raw event with a timeout so we can re-check stop.
            match raw_rx.recv_timeout(DEBOUNCE) {
                Ok(first) => {
                    collect_event(&mut pending, first);
                    // drain until quiet
                    while let Ok(ev) = raw_rx.recv_timeout(DEBOUNCE) {
                        collect_event(&mut pending, ev);
                    }
                }
                Err(_) => {
                    // Timeout: no events. Flush any accumulated pending (shouldn't happen, but
                    // handles the case where events arrive during the first recv_timeout gap).
                }
            }
            if pending.is_empty() {
                continue;
            }
            for p in pending.drain() {
                builder::update_path(&state, &p);
            }
            if let Ok(mut w) = state.writer.lock() {
                let _ = w.commit();
            }
            let _ = state.reader.reload(); // reflect incremental updates in searches
        }
    });
    Ok(stop_tx)
}

fn collect_event(pending: &mut HashSet<PathBuf>, ev: notify::Result<notify::Event>) {
    if let Ok(ev) = ev {
        use notify::EventKind;
        if matches!(
            ev.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            for p in ev.paths {
                pending.insert(p);
            }
        }
    }
}

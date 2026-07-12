# Changelog

## 0.4.0

### Rename

- **indexify → loupe.** The binary, VS Code extension, index directory (`.loupe/`), VS Code settings
  keys (`loupe.*`), and documentation now use the **loupe** name consistently.
- **Marketplace id `ukitomato.loupe-search`.** The extension package name is `loupe-search` (display
  name **Loupe Search**) because the short name `loupe` is already taken on the Marketplace.

### CLI / core

- **Read-only mode when the index is locked.** If another process (e.g. `loupe serve`) holds the
  Tantivy writer lock, `loupe search` and the MCP server open the index read-only and continue
  serving searches instead of failing with `LockBusy`. Build/sync remain unavailable until the
  writer is released.

## 0.3.0

### CLI / core

- **2-character search.** The index now stores **bigrams as well as trigrams**, so 2-char
  substring queries work — most useful for 2-char Japanese words (e.g. `契約`, `顧客`) that were
  previously unsearchable. 1-char queries remain unsupported (nothing to pre-filter on).
- **Regex pre-filter accepts ≥2-char literal runs, including CJK.** A pattern like `契約.*情報`
  now uses the index instead of erroring; previously only ASCII literal runs of ≥3 chars counted.
- **Incomplete-result notice.** When a query's candidate set hits the internal cap (so some
  matching files were not verified — most likely for a very common short query), the CLI prints a
  note on stderr and the MCP / sidecar responses flag it, instead of silently returning a partial set.

> **Rebuild required:** the index format changed (bigrams added). Run `loupe build --force`
> once after upgrading. A plain `sync` only reindexes changed files, so it will *not* backfill
> bigrams into an existing index.

## 0.2.0

### VS Code extension

- **Sidebar search view** (Activity Bar → loupe icon, `Ctrl+Alt+Shift+F`) — persistent panel with
  streaming results grouped by file, match highlighting, and file-at-line navigation.
  - File group headers show the filename on its own line with the directory path below; hover the
    header to see the full path as a tooltip.
  - **`Aa`** case-sensitive toggle.
  - **`.*`** regular-expression toggle.
  - **Max results** dropdown — 50 / 100 / 300 / 1000 / ∞.
  - **`···`** reveals path filter fields:
    - **Files to include** — glob patterns to restrict results (e.g. `src/`, `*.java`).
    - **Files to exclude** — glob patterns to hide results (e.g. `*.min.js`, `test/`).
    - Both support `*` (within segment), `**` (across segments), `?` (single char), or plain
      substring. Filters are applied client-side without re-searching.

### CLI / core

- **`--case-sensitive`** flag for `loupe search` — exact-case substring and regex matching.
  The trigram phase still uses lowercase for fast candidate selection; the verify step re-checks
  original bytes when `--case-sensitive` is set.

### CI

- `vscode.yml`: changed trigger from `release: published` to `workflow_run` to work around the
  GitHub Actions restriction that `GITHUB_TOKEN`-created releases do not cascade to other workflows.

---

## 0.1.0

Initial release.

- Fast indexed full-text code search over a Tantivy trigram index.
- One binary, three front-ends sharing a single index: a **CLI**
  (`init` / `build` / `sync` / `search` / `status`), an **MCP stdio server** (`mcp`), and an NDJSON
  **sidecar** (`serve`) for the VS Code extension.
- Roots and per-folder encodings are configured in `settings.json` — the single source of truth read
  by all three front-ends.
- Per-folder encoding decoded at index time: **UTF-8, Shift_JIS, and EUC-JP** coexist in one index.
- **Substring and regex** search: trigram candidates → exact verify (parallel).
- Search **auto-syncs** changed files first; a filesystem watcher keeps the index fresh incrementally.
- Index stored in `<workspace>/.loupe/` (`settings.json` is committable; the index body is
  git-ignored).
- Native binaries per platform under `bin/<os>-<arch>/`; no Docker, no runtime dependencies.

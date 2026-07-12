<div align="center">

# loupe

**A developer tool for fast indexed full-text code search — one n-gram index, three front-ends: CLI, MCP server, and VS Code.**

A small Rust ([Tantivy](https://github.com/quickwit-oss/tantivy)) binary keeps a compact n-gram index,
decoding **UTF-8, Shift_JIS, and EUC-JP** per folder, and updates it incrementally as files change.

![CLI](https://img.shields.io/badge/CLI-loupe-DEA584?logo=rust&logoColor=white)
![MCP](https://img.shields.io/badge/MCP-stdio%20server-blue)
![VS Code](https://img.shields.io/badge/VS%20Code-extension-007ACC?logo=visualstudiocode&logoColor=white)
![Engine](https://img.shields.io/badge/engine-Rust%20%2F%20Tantivy-DEA584?logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-green)

</div>

---

**loupe** is a developer productivity tool: a source-code search engine that software engineers run
locally (via the CLI, an editor extension, or an AI coding agent) to find code across their own
repositories. Plain recursive grep re-scans the whole tree on every query, editor search crawls on
big projects, and most code-search tools assume everything is UTF-8. loupe trades a one-time index
build for near-instant searches afterward, and decodes each folder by its own encoding so legacy
non-UTF-8 sources are searchable too — **Docker-free, no runtime deps**.

It works on any project, and it shines where search usually hurts: **large or multi-encoding
codebases** — for example a monorepo holding many repositories, or a tree mixing modern UTF-8 code with
legacy Shift_JIS assets.

- ⚡ **Compact n-gram index** — a small fraction of your code size, not a copy of it.
- 🈶 **Per-folder encoding** — each folder is decoded (UTF-8 / Shift_JIS / EUC-JP …) at index time, so a
  single index serves mixed-encoding trees and non-UTF-8 text is searchable without mojibake.
- 🔁 **Incremental** — search auto-syncs changed files first; the daemon/extension also watch the tree and
  reindex only what changed, so the index stays fresh without re-scanning everything.
- 🔎 **Substring and regex** — n-gram candidates → exact verify (Zoekt/codesearch style); 2-char queries work (incl. 2-char CJK words).
- 🔡 **Case-sensitive or case-insensitive** — your choice per query.
- 🧩 **One index, three front-ends** — the **CLI**, an **MCP server** (for AI agents), and the **VS Code**
  extension all read the same index and the same `settings.json`, so they can never disagree about what's
  indexed.
- 🪶 **Self-contained native binary** — one `loupe` executable per platform, no Docker, no runtime deps.

## 🧠 Model

Three steps, separated on purpose:

1. **`init`** — choose which folders to index and each folder's encoding. Writes
   `<index-dir>/settings.json` — the single source of truth shared by all front-ends.
2. **`build`** — create the index from `settings.json`.
3. **`search`** — the everyday operation; it auto-syncs changed files first.

The index lives in `<workspace>/.loupe/` by default (override with `--index-dir` or
`LOUPE_INDEX_DIR`). `settings.json` is safe to commit; the index body (`tantivy/`, `meta.json`) is
git-ignored automatically.

## 📦 Install

Prebuilt binaries for Linux, macOS, and Windows are published to GitHub Releases (built by
[cargo-dist](https://github.com/axodotdev/cargo-dist)). One-line installers:

```bash
# Linux / macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ukitomato/loupe/releases/latest/download/loupe-installer.sh | sh
```

```powershell
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/ukitomato/loupe/releases/latest/download/loupe-installer.ps1 | iex"
```

Or grab a tarball from the [Releases page](https://github.com/ukitomato/loupe/releases), or build
from source: `cargo install --git https://github.com/ukitomato/loupe loupe`.

Windows binaries are intended to be signed for free by the [SignPath Foundation](https://signpath.org/)
(application pending — see [Code Signing Policy](#-code-signing-policy)).

## 🚀 CLI

```bash
# 1. configure roots (interactive in a terminal, or via flags); add @enc for non-UTF-8 folders
loupe init --root src --root lib
loupe init --root src --root legacy@shift_jis      # mixed encodings in one index

# 2. build the index from settings.json
loupe build

# 3. search (auto-syncs first)
loupe search "calcTotal"
loupe search "calcTotal" --case-sensitive           # exact case
loupe search "parse[A-Za-z]+Request" --regex
loupe search "parseRequest" --regex --case-sensitive
loupe search "calcTotal" --max 50 --json            # JSON array of { file, line, text }

loupe status                                        # built? file count, roots, last build/sync
```

| Command | Purpose |
| --- | --- |
| `init [--root PATH[@ENC]]… [--force]` | Configure roots/encodings → `settings.json` |
| `build [--force]` | (Re)build the index from `settings.json` |
| `sync` | Incremental catch-up (search does this automatically) |
| `search <q> [--regex] [--case-sensitive] [--max N] [--json] [--no-sync]` | Search the index |
| `status [--json]` | Index statistics |
| `serve` | NDJSON daemon used by the VS Code extension |
| `mcp` | MCP (Model Context Protocol) stdio server |

## 🤖 MCP server (AI agents)

`loupe mcp` speaks the Model Context Protocol over stdio. Register it with your MCP client:

```jsonc
{
  "mcpServers": {
    "loupe": {
      "command": "/path/to/loupe",
      "args": ["mcp", "--index-dir", "/path/to/workspace/.loupe"]
    }
  }
}
```

Tools exposed: `search_code`, `search_regex`, `build_index`, `sync_index`, `index_status`. The server
opens the shared index and keeps it fresh via a file watcher for the lifetime of the session.

## 🧩 VS Code extension

Install **[Loupe Search](https://marketplace.visualstudio.com/items?itemName=ukitomato.loupe-search)** from the Marketplace.

The extension bundles the `loupe` binary and shares the same index and `settings.json` as the CLI and MCP server. Two search UIs are included:

- **Sidebar** — Activity Bar icon or `Ctrl+Alt+Shift+F`; streaming results grouped by file.
- **QuickPick** — `Ctrl+Alt+F` for a one-shot search dropdown.

Full usage, screenshots, commands, and VS Code settings are documented in [`vscode/README.md`](vscode/README.md) (also shown on the Marketplace page).

## ⚙️ Configuration — `settings.json`

`<index-dir>/settings.json` is the one place that defines what gets indexed:

```jsonc
{
  "roots": [
    { "path": "src",    "encoding": "utf-8" },
    { "path": "assets", "encoding": "shift_jis" }
  ]
}
```

Write it with `loupe init`, or edit it by hand. Relative paths resolve against the workspace root
(the parent of the index dir).

## 🔧 How it works

```
   CLI / MCP server / VS Code extension
     │  (all read settings.json + the same index)
     ▼
   loupe  (Rust / Tantivy)
     ├─ build:   parallel walk → per-file decode (UTF-8/Shift_JIS/EUC-JP) → DISTINCT bigrams+trigrams → index
     ├─ sync:    compare mtimes → reindex only changed files, drop deleted ones
     ├─ watch:   notify FS events → incremental update (delete+add, debounced)
     └─ search:  n-gram-AND candidates → parallel verify (substring/regex, case-sensitive option)
```

## 📊 Measured (≈290k files: ~260k UTF-8 + ~29k Shift_JIS)

| | |
| --- | --- |
| Index size | **≈237 MB** |
| First build (cold, one-time) | ~28 min · then incremental is instant |
| Search — specific identifier | ~180 ms |
| Search — Japanese in Shift_JIS | ~156 ms |
| Search — very common term | <1 s |

## 📋 Notes

- **Binaries** are distributed via GitHub Releases (built by cargo-dist), not committed to the repo.
  The VS Code extension's CI downloads the matching one and bundles it under `bin/<os>-<arch>/` at
  package time; for local development, `cargo build` and point `loupe.binaryPath` (or `$PATH`) at it.
- **regex** uses the index only when the pattern contains a literal run of ≥2 characters, ASCII or CJK (e.g. `func\s+\w+`, `契約.*情報`).
- **Case-sensitive** mode: the n-gram phase always over-approximates with lowercase n-grams, then the
  verify step re-checks original bytes for exact case — so case-sensitive searches are safe but slightly
  slower on large candidate sets.
- If antivirus scans the index directory, builds can occasionally hit a transient I/O error; loupe
  retries automatically. Excluding the index folder from AV avoids it entirely.

## 🔏 Code Signing Policy

We are applying to the [SignPath Foundation](https://signpath.org/) program for free Windows code
signing. **Status: pending approval.** Once approved, the statement required by the program will be:
"Free code signing provided by SignPath.io, certificate by SignPath Foundation".

**What will be signed:** Windows binaries (`loupe.exe`), built from this public repository by GitHub
Actions (`cargo-dist`, see [`.github/workflows/release.yml`](.github/workflows/release.yml)) and
distributed in two forms — the standalone CLI archive/installer on
[GitHub Releases](https://github.com/ukitomato/loupe/releases), and the copy bundled into the
`win32-x64` VS Code extension `.vsix` (see [`.github/workflows/vscode.yml`](.github/workflows/vscode.yml)).
The `.vsix` reuses the exact same released binary; there is no separate build path, so only one
signed artifact needs to be produced per release.

- **Authors (commit access):** [ukitomato](https://github.com/ukitomato) (sole maintainer).
- **Reviewers (review required for non-committer changes):** [ukitomato](https://github.com/ukitomato) —
  all external pull requests are reviewed by the maintainer before merge.
- **Approvers (approve each signing request):** [ukitomato](https://github.com/ukitomato) — each
  signing request requires explicit approval by the maintainer.
- **Privacy:** This program will not transfer any information to other networked systems unless
  specifically requested by the user. `loupe` builds and searches its index entirely on your local
  filesystem and makes no network calls.

## 📄 License

MIT — see the `LICENSE` file. Built on [Tantivy](https://github.com/quickwit-oss/tantivy) (MIT).

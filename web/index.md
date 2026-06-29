---
layout: home

hero:
  name: "indexify"
  text: "Fast indexed full-text code search"
  tagline: "One n-gram index. Three front-ends: CLI, MCP server, and VS Code. UTF-8, Shift_JIS, and EUC-JP — all in one index."
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/ukitomato/indexify

features:
  - icon: ⚡
    title: Compact n-gram index
    details: A small fraction of your code size, not a copy of it. One-time build, then near-instant searches — no re-scan on every query.

  - icon: 🈶
    title: Per-folder encoding
    details: Each folder is decoded (UTF-8 / Shift_JIS / EUC-JP) at index time, so a single index serves mixed-encoding trees and legacy non-UTF-8 text is searchable without mojibake.

  - icon: 🔁
    title: Incremental updates
    details: Search auto-syncs changed files first. The daemon and VS Code extension watch the tree and reindex only what changed — the index always stays fresh.

  - icon: 🔎
    title: Substring and regex
    details: n-gram candidates → exact verify (Zoekt/codesearch style). 2-char queries work, including 2-char CJK words like 契約 and 顧客. Case-sensitive or case-insensitive, your choice.

  - icon: 🧩
    title: One index, three front-ends
    details: The CLI, MCP server (for AI agents), and VS Code extension all read the same index and the same settings.json — they can never disagree about what's indexed.

  - icon: 🪶
    title: Self-contained native binary
    details: One indexify executable per platform. No Docker, no runtime dependencies, no daemon required for the CLI. Runs anywhere.
---

## VS Code Extension

Install from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=ukitomato.indexify) — or grab a `.vsix` from [GitHub Releases](https://github.com/ukitomato/indexify/releases).

The extension ships **two search UIs** that share the same index as the CLI and MCP server, updated automatically in the background.

**Sidebar** (`Ctrl+Alt+Shift+F`) — persistent panel with streaming results grouped by file, match highlighting, case-sensitive toggle (`Aa`), regex toggle (`.*`), max-results dropdown, and glob path filters.

**QuickPick** (`Ctrl+Alt+F`) — lightweight one-shot search into a filterable dropdown.

[VS Code Extension reference →](/reference/vscode)

---

## Install

Prebuilt binaries for Linux, macOS, and Windows are on [GitHub Releases](https://github.com/ukitomato/indexify/releases).

```bash
# Linux / macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ukitomato/indexify/releases/latest/download/indexify-installer.sh | sh
```

```powershell
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/ukitomato/indexify/releases/latest/download/indexify-installer.ps1 | iex"
```

Or build from source:

```bash
cargo install --git https://github.com/ukitomato/indexify indexify
```

## Quick start

```bash
# 1. Configure roots (add @enc suffix for non-UTF-8 folders)
indexify init --root src --root legacy@shift_jis

# 2. Build the index
indexify build

# 3. Search — auto-syncs first
indexify search "calcTotal"
indexify search "契約" --case-sensitive
indexify search "parse[A-Za-z]+Request" --regex
```

[Full guide →](/guide/getting-started)

## Performance

Measured on ≈290k files (~260k UTF-8 + ~29k Shift_JIS):

| | |
|---|---|
| Index size | **≈237 MB** |
| First build (one-time, cold) | ~28 min |
| Search — specific identifier | ~180 ms |
| Search — Japanese in Shift_JIS | ~156 ms |
| Search — very common term | < 1 s |

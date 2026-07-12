# VS Code Extension

The loupe VS Code extension provides two search UIs that share the same index as the CLI and MCP server. The index is updated automatically in the background via a filesystem watcher.

---

## Install

The extension is published on the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=ukitomato.loupe-search).

You can also install a `.vsix` directly from the [GitHub Releases page](https://github.com/ukitomato/loupe/releases).

::: tip Prerequisite
You need to run `loupe init` and `loupe build` at least once before using the extension. The extension does not build the index automatically — use the command palette (`loupe: Build / rebuild index`) or the CLI.
:::

---

## Sidebar view

Click the **loupe icon** in the Activity Bar, or press **`Ctrl+Alt+Shift+F`** to focus the Sidebar search panel.

### Results

- Results are **grouped by file**. Each file header shows the filename prominently, with the directory path below. Hover the header to see the full path as a tooltip.
- Each result row shows the matched line with **match highlighting**. Click a row to open the file at that line.
- Results **stream in** progressively as they arrive from the sidecar process.

### Toolbar controls

| Control | Description |
|---|---|
| **`Aa`** | Case-sensitive toggle. Default: case-insensitive. |
| **`.*`** | Regular expression toggle. Requires a literal run of ≥ 2 characters in the pattern. |
| **Max results** | Dropdown: 50 / 100 / 300 / 1000 / ∞. Takes effect on the next search. |
| **`···`** | Expand path filter fields (see below). |

### Path filters

Click **`···`** to reveal two filter fields:

| Field | Description |
|---|---|
| **Files to include** | Only show results from matching paths. Examples: `src/`, `*.java`, `delivery/**/*.java` |
| **Files to exclude** | Hide results from matching paths. Examples: `*.min.js`, `test/` |

Both fields accept glob patterns:
- `*` — matches within a single path segment
- `**` — matches across path segments
- `?` — matches a single character
- Plain text without wildcards — treated as a substring of the path

Filters are applied **client-side** — changing a filter does not trigger a new search, it just re-filters the existing results.

---

## QuickPick

Press **`Ctrl+Alt+F`** for a lightweight one-shot search. Results stream into a filterable dropdown; press `Enter` to open the selected file at the matched line.

---

## Commands

| Command | Keybinding | Description |
|---|---|---|
| **loupe: Search (substring)** | `Ctrl+Alt+F` | QuickPick — substring search |
| **loupe: Search (regex)** | — | QuickPick — regex search |
| **loupe: Focus Search View** | `Ctrl+Alt+Shift+F` | Focus the sidebar search panel |
| **loupe: Build / rebuild index** | — | Full (re)build of the index |

---

## VS Code settings

| Setting | Default | Description |
|---|---|---|
| `loupe.indexDir` | `.loupe` (workspace root) | Path to the index directory. Relative paths resolve from the workspace root. |
| `loupe.binaryPath` | (auto-detect from PATH) | Path to the `loupe` binary. Set this if the binary is not on `$PATH`. |
| `loupe.maxResults` | `100` | Default maximum results for the QuickPick search. |

::: warning Roots and encodings are not VS Code settings
Which folders are indexed and their encodings are configured in `settings.json` (via `loupe init` or by hand). This keeps the CLI, MCP server, and extension in agreement. VS Code settings only control the editor-side experience.
:::

---

## How the extension works

The extension spawns `loupe serve` as a sidecar subprocess. The sidecar:
- Opens the shared on-disk index
- Starts a filesystem watcher to reindex changed files automatically
- Accepts NDJSON requests over stdin and streams NDJSON responses to stdout

The extension communicates with the sidecar over stdio, so no network port is used.

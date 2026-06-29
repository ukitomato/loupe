# MCP Server

`indexify mcp` speaks the [Model Context Protocol](https://modelcontextprotocol.io/) over stdio. It lets any MCP-compatible AI agent (Claude, Cursor, etc.) search your codebase through the same indexed engine used by the CLI and VS Code extension.

The server opens the shared index, starts a filesystem watcher, and keeps the index fresh for the lifetime of the session.

---

## Registration

Add the following to your MCP client's configuration file:

```jsonc
{
  "mcpServers": {
    "indexify": {
      "command": "/path/to/indexify",
      "args": ["mcp", "--index-dir", "/path/to/workspace/.indexify"]
    }
  }
}
```

Replace `/path/to/indexify` with the actual binary location (find it with `which indexify`), and `/path/to/workspace/.indexify` with the index directory for your project.

::: tip Default index directory
If you omit `--index-dir`, the server looks for `.indexify/` relative to the working directory it is launched from. Configure `--index-dir` explicitly if you run the MCP server from a different directory.
:::

### Claude Code example

```jsonc
// .claude/mcp.json  (or ~/.claude/mcp.json for global)
{
  "mcpServers": {
    "indexify": {
      "command": "indexify",
      "args": ["mcp", "--index-dir", "/path/to/your/project/.indexify"]
    }
  }
}
```

---

## Exposed tools

### `search_code`

Substring search over the index.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `query` | string | yes | The substring to search for (≥ 2 characters). |
| `case_sensitive` | boolean | no | Default `false`. |
| `max_results` | integer | no | Maximum results to return. Default: 100. |

**Returns:** An array of `{ file, line, text }` objects, and a boolean `incomplete` flag when the result may be truncated.

---

### `search_regex`

Regular expression search over the index.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pattern` | string | yes | Regex pattern. Must contain a literal run of ≥ 2 characters. |
| `case_sensitive` | boolean | no | Default `false`. |
| `max_results` | integer | no | Maximum results to return. Default: 100. |

**Returns:** Same shape as `search_code`.

---

### `build_index`

(Re)build the index from `settings.json`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `force` | boolean | no | If `true`, delete and fully rebuild. Default `false`. |

---

### `sync_index`

Incrementally sync the index (reindex changed files, remove deleted files).

No parameters.

---

### `index_status`

Return current index statistics.

No parameters.

**Returns:** `{ built: boolean, file_count: number, roots: [...], last_build: string, last_sync: string }`

---

## Notes

- The MCP server maintains a **filesystem watcher** for the duration of the session; the index is updated in the background as files change.
- `search_code` and `search_regex` set the `incomplete` flag (instead of silently truncating) when the candidate set hits the internal cap.
- Roots and encodings are read from `settings.json` — there are no MCP-level configuration parameters for indexing scope.

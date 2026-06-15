# TokenLens — Architecture

This document describes the runtime architecture of TokenLens.

## Goals

1. Answer: how many AI tokens am I using, where are they going, what do they cost?
2. Run entirely on the user's machine. No AI calls in the analytics path. No cloud.
3. Stay out of the way of upstream tools (OpenCode, Veyra, LM Studio, etc.) — passive or
   opt-in light-weight integration only.
4. Be correct about exactness — label data as `exact`, `estimated`, `mixed`, or `unknown`.

## Process topology

TokenLens is a single Tauri process. The Tauri shell runs a webview that hosts the React
UI. The Rust backend is the source of truth for the database, parsing, and aggregation.

```
┌──────────────────────────────────────────┐
│            Tauri Main Process            │
│  ┌────────────────┐  ┌────────────────┐  │
│  │   Webview      │  │  Rust Backend  │  │
│  │   (React/TS)   │◄─┤  - db (sqlite) │  │
│  │                │  │  - ingest      │  │
│  │                │  │  - pricing     │  │
│  │                │  │  - watcher     │  │
│  │                │  │  - settings    │  │
│  └────────────────┘  └────────────────┘  │
└──────────────────────────────────────────┘
```

## Data pipeline

```
   +-----------+        +----------------+        +---------------+        +---------+
   |  Sources  |  --->  |  Collectors    |  --->  |  Normalizer   |  --->  |   DB    |
   +-----------+        +----------------+        +---------------+        +---------+
        |                      |                        |                       |
        |                      |                        |                       v
        |                      |                        |                  +---------+
        |                      |                        |                  |  UI/TS  |
        |                      |                        |                  +---------+
        v                      v                        v
   - opencode logs         - scanner (walkdir)     - provider detection
   - jsonl inbox           - watcher (notify)      - model extraction
   - veyra  (P5)           - jsonl inbox reader    - session/project id
   - lmstudio (P5)         - oai proxy (P5)        - usage field extraction
   - oai proxy (P5)                                - dedup hash
                                                  - secret redaction
                                                  - cost computation
```

## Module boundaries

| Module | Responsibility | Touches |
| --- | --- | --- |
| `commands` | Tauri command surface, request shaping | All other modules |
| `db` | Connection, migrations, helpers | `types`, `errors` |
| `ingest` | File parser, normalizer, dedup, persist | `pricing`, `redaction`, `db` |
| `aggregation` | Pure read queries against `usage_events` | `db`, `types` |
| `collectors` | Per-source adapters (opencode, veyra, lmstudio, jsonl inbox) | `ingest`, `db` |
| `watcher` | File system watcher (notify-debouncer) | `ingest` |
| `pricing` | Cost engine, seeded defaults, history | `db` |
| `redaction` | Secret stripping | (none) |
| `settings` | Key-value settings store | `db` |
| `token_estimator` | chars/4 fallback | (none) |
| `types` | Shared types | `serde`, `chrono` |
| `errors` | AppError + serialization | `serde` |

## Threading

- The Tauri main process holds a single `Arc<Mutex<Connection>>` to SQLite. All
  reads and writes serialize through the mutex. WAL mode allows concurrent reads
  at the SQLite level, but our `parking_lot::Mutex` simplifies the Rust API.
- Watcher events arrive on `notify`'s background thread and are forwarded to a
  `tokio::sync::mpsc` channel. A `tokio::spawn`-ed task consumes the channel and
  parses changed files.
- All Tauri commands are async by convention; sync commands block the
  async-runtime worker briefly while the SQLite work runs (typically <1 ms).

## Configuration

- **DB path**: `<app local data>/TokenLens/tokenlens.sqlite`
  - Windows: `%LOCALAPPDATA%\TokenLens\`
  - macOS: `~/Library/Application Support/TokenLens/`
  - Linux: `~/.local/share/TokenLens/`
- **Logs**: `<app data>/TokenLens/logs/tokenlens.log.YYYY-MM-DD` (daily rotation).
- **Inbox**: `<app local data>/TokenLens/inbox/opencode.jsonl` (consumed by
  the OpenCode plugin).

## Failure modes

| Failure | Behavior |
| --- | --- |
| OpenCode log format changes | Normalizer falls back to deep key search; unparseable lines are skipped silently. Raw JSON is preserved so re-parse is possible later. |
| Missing pricing for a model | Cost is recorded as 0. UI shows "(no price)" warning in Settings. |
| Watcher misses a write | File offsets are stored per-source-per-file. A re-scan picks up missed bytes. |
| DB corruption | Recovery via `vacuum_db` (best effort) or `reset_all_data` (nuclear). |
| Disk full | Writes fail with `Io` error; UI surfaces it via toast. |

## Privacy & isolation

- The app cannot reach the network. Tauri capabilities scope `fs` to user data
  dirs only. No HTTP client is included.
- The OpenCode plugin (optional) writes to a local inbox file only.
- Raw JSON in the DB is opt-out (default: on, with secret redaction).
- No analytics, no update checks, no crash reporting.

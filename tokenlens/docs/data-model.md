# TokenLens — Data Model

This document describes the SQLite schema in `src-tauri/migrations/001_init.sql`
and the contract between Rust and the TypeScript frontend.

## Tables

### `sources`

A source is a place we read events from. We never write to a source path.

| Column | Type | Description |
| --- | --- | --- |
| id | INTEGER PK | |
| name | TEXT NOT NULL | Human label |
| kind | TEXT NOT NULL | `opencode_logs`, `opencode_inbox`, `veyra`, `lmstudio`, `oai_proxy`, `manual` |
| path | TEXT | Filesystem path or null |
| enabled | INT (0/1) | |
| last_scanned_at | TEXT (ISO 8601) | |
| last_error | TEXT | |
| created_at | TEXT | |

### `projects`

A project is a workspace / repo / folder we associate events with. Resolution
happens at ingest time via `cwd`, `workspace`, `project_path` fields in source
events.

| Column | Type | Description |
| --- | --- | --- |
| id | INTEGER PK | |
| name | TEXT | |
| path | TEXT UNIQUE | |
| git_remote | TEXT | |
| created_at | TEXT | |

### `sessions`

A session is a top-level conversation. Identity is `(source_id, source_session_id)`.

| Column | Type | Description |
| --- | --- | --- |
| id | INTEGER PK | |
| source_session_id | TEXT NOT NULL | From the source |
| source_id | INT FK → sources | |
| project_id | INT FK → projects | |
| title | TEXT | |
| started_at, last_seen_at | TEXT | |
| provider, model | TEXT | |
| total_tokens | INT | Running sum |
| total_cost_usd | REAL | Running sum |
| exactness | TEXT | `exact`/`estimated`/`mixed`/`unknown` |
| raw_ref | TEXT | Optional pointer to source artifact |
| UNIQUE(source_id, source_session_id) | | |

### `usage_events`

The heart of the system. One row per message / step / tool call.

| Column | Type | Description |
| --- | --- | --- |
| id | INTEGER PK | |
| event_hash | TEXT UNIQUE | SHA-256 dedup key |
| timestamp | TEXT NOT NULL (ISO 8601) | |
| source_id, session_id, project_id | INT FK | |
| event_type | TEXT | `message`, `step`, `tool_call`, `session_start`, etc. |
| provider, model | TEXT | |
| message_role | TEXT | `user`/`assistant`/... |
| input_tokens, output_tokens | INT | |
| reasoning_tokens | INT | |
| cache_read_tokens, cache_write_tokens | INT | |
| tool_tokens | INT | |
| total_tokens | INT | `input + output` if not set explicitly |
| cost_usd | REAL | Computed at insert from `model_pricing` |
| exactness | TEXT | |
| confidence | REAL | 0.0–1.0 |
| raw_json | TEXT | Optional, post-redaction |
| raw_source_path | TEXT | |
| ignored | INT (0/1) | Soft-delete flag |
| created_at | TEXT | |

Indexes: `timestamp`, `session_id`, `model`, `project_id`, `provider`, `exactness`, `ignored`.

### `model_pricing`

Per-(provider, model) pricing. Editable from Settings.

| Column | Type | Description |
| --- | --- | --- |
| id | INTEGER PK | |
| provider, model | TEXT NOT NULL | |
| input/output/reasoning/cache_read/cache_write_price_per_million | REAL | USD per 1M tokens |
| currency | TEXT | Default `USD` |
| effective_date | TEXT | Optional |
| is_local | INT (0/1) | If 1, cost is always 0 |
| source | TEXT | `seed` / `manual` / `api` |
| updated_at | TEXT | |
| UNIQUE(provider, model) | | |

### `pricing_history`

Append-only log of pricing changes. Populated on every `upsert_pricing` call
that mutates an existing row.

### `daily_usage`

Materialized daily rollup. Rebuilt on demand via `rebuild_daily_aggregates`.

PK: `(date, provider, model, project_id)`.

### `alerts`

Out-of-band notifications (daily limit, monthly cost, context full, etc.).

### `settings`

Key-value bag. Keys map to fields on `AppSettings`.

### `file_offsets`

Per-source-per-file byte offset so the watcher can pick up where it left off.

### `inbox_files`

Tracks JSONL inbox files seen by the collector. Processed files are moved to
`inbox/archive/`.

## Frontend contracts

`src/types/contracts.ts` mirrors the Rust structs. The Tauri command surface
in `src/lib/tauri.ts` returns the same shape.

| Frontend type | Rust type |
| --- | --- |
| `Source` | `types::Source` |
| `Session` | `types::Session` |
| `UsageEvent` | `types::UsageEvent` |
| `ModelPricing` | `types::ModelPricing` |
| `OverviewStats` | `types::OverviewStats` |
| `TimeseriesPoint` | `types::TimeseriesPoint` |
| `Breakdown` | `types::Breakdown` |
| `QueryFilter` | `types::QueryFilter` |
| `ScanResult` | `types::ScanResult` |
| `AppSettings` | `settings::AppSettings` |
| `AppError` | `errors::AppError` (serialized as `{kind, message}`) |

## Storage estimates

| Mode | Bytes / month |
| --- | --- |
| Minimal (totals only, raw off) | 1–5 MB |
| Detailed (per-message rows, raw on, 14-day raw) | 10–100 MB |
| Debug (raw on, full retention) | 100 MB – 2+ GB |

# TokenLens

**Local-first AI token and cost analytics.** A Tauri v2 desktop app for understanding how many AI tokens you use, where they go, and what they cost. Runs entirely on your machine. No cloud. No AI calls. No telemetry.

```
+------------------------+        +-------------------------+        +----------------------+
| Sources (logs, JSONL)  |  --->  | Collectors + Normalizer |  --->  | SQLite (events, etc) |
+------------------------+        +-------------------------+        +----------------------+
                                                                              |
                                                                              v
                                                                        +--------------+
                                                                        | Tauri UI     |
                                                                        | (React+TS)   |
                                                                        +--------------+
```

## Quick start

```bash
cd tokenlens
npm install
npm run tauri:dev
```

That opens a desktop window. Click **Samples** in the topbar to populate the dashboard with synthetic data, or add a real source under **Settings → Sources**.

For a full release build (installers, msi, etc.):

```bash
npm run tauri:build
```

The output binary lives in `src-tauri/target/release/`.

## Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri v2 |
| Backend | Rust (tokio, rusqlite, notify, serde, tracing) |
| DB | SQLite (WAL mode, bundled) |
| Frontend | React 18 + TypeScript + Vite |
| UI | Tailwind CSS + hand-rolled shadcn-style components |
| Charts | Recharts |
| State | Zustand + persist |
| Optional plugin | TypeScript (no deps, JSONL-only) |

## Architecture

The app follows the three-stage pipeline from the original plan:

1. **Sources** — log folders, JSONL inbox, or a local OAI-compatible proxy. TokenLens never writes to your source folders.
2. **Collectors + Normalizer** — file scanner, file watcher, JSONL inbox scanner. Each event is normalized to a canonical `UsageEvent`, SHA-256-hashed, and persisted.
3. **SQLite** — events, sessions, projects, model_pricing, daily_usage, alerts, settings. WAL mode for concurrent reads.

## Privacy

- **No network calls.** No telemetry. No cloud sync. No AI calls in the analytics path.
- **No full message storage** by default. Token counts and metadata only.
- **Secret redaction** strips obvious API keys (OpenAI, Anthropic, Google, GitHub, AWS, JWT, private keys) before raw JSON is persisted.
- **Path anonymization** is opt-in in Settings → Privacy.
- **Reset all data** is a one-click operation.

## Storage strategy

| Level | Stored |
| --- | --- |
| Always | Token counts, model/provider/session/project, dates, event hashes, cost estimates |
| 7–30 days | Raw JSON (configurable) |
| Off by default | Full prompts/responses, terminal output, code diffs, env vars |

## Cost estimates

Costs are computed from the **pricing table** (Settings → Pricing). We seed a default table for popular models (gpt-4o, claude-sonnet-4-5, gemini-2.5-pro, …) and you can edit, add, or remove entries. Local models are $0 by default. A "Recalculate costs" button reapplies current pricing to all events.

Costs are **estimates**. We do not pretend to bill on your behalf.

## Token estimation

When source data lacks exact counts, TokenLens uses a `chars / 4` heuristic by default. The estimator mode is configurable in Settings. Exact-mode events from the source data are preferred and labeled as such.

## OpenCode integration

Three ways to bring OpenCode data in:

1. **Passive import (default)** — point TokenLens at `~/.local/share/opencode/log` (or the Windows equivalent) and it scans. No plugin needed.
2. **Live watcher** — same source, plus a background file watcher.
3. **Optional plugin** — drop `collectors/opencode-plugin/tokenlens.ts` into your OpenCode project. It appends one small JSONL line per event to a TokenLens inbox. TokenLens scans the inbox and archives processed files.

## Project structure

```
tokenlens/
├─ src/                       React + TS frontend
│  ├─ app/
│  ├─ components/             layout, ui primitives, cards
│  ├─ pages/                  Overview, Sessions, Projects, Models, …
│  ├─ charts/                 Recharts wrappers
│  ├─ stores/                 Zustand stores
│  ├─ lib/                    tauri invoke wrapper, utils, mock backend
│  └─ types/                  TypeScript contracts
├─ src-tauri/                 Rust backend
│  ├─ src/
│  │  ├─ commands/            Tauri command surface
│  │  ├─ db/                  SQLite + migrations
│  │  ├─ ingest/              parser, normalizer, dedup
│  │  ├─ aggregation/         queries (overview, breakdowns, sessions)
│  │  ├─ collectors/          opencode, veyra, lmstudio, jsonl inbox
│  │  ├─ watcher/             notify-based file watcher
│  │  ├─ pricing/             cost engine + seeded defaults
│  │  ├─ redaction/           secret stripping
│  │  ├─ settings/            app settings
│  │  ├─ token_estimator/     chars/4 fallback
│  │  └─ types.rs             shared types
│  ├─ migrations/             SQL migrations
│  └─ tests/fixtures/         sample log data
├─ collectors/opencode-plugin/   tiny TypeScript plugin
└─ docs/                      architecture, data model, privacy
```

## Testing

```bash
cd src-tauri && cargo test --lib     # 17 unit tests for normalizer, dedup, redaction, estimator
```

## Roadmap (per plan.md)

- [x] Phase 1 — MVP: passive import, dashboard, sessions, models
- [x] Phase 2 — Live collection, settings, storage cleanup, exports
- [x] Phase 3 — OpenCode plugin + JSONL inbox
- [ ] Phase 4 — Budget alerts, forecasting, advanced filters
- [ ] Phase 5 — Veyra + LM Studio + OAI-compatible proxy

## License

MIT — see `LICENSE`.

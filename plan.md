Main goal

The app should answer:

How many tokens am I using?
Where are they going?
Which model/project/session is expensive?
How much would this cost?
How much context am I wasting?
What tools/agents are causing token spikes?

It should run locally, use zero extra AI tokens by default, and only use disk/CPU/RAM.

2. Recommended stack

Since you already work with Tauri/Veyra, use this:

Desktop shell:
- Tauri v2

Backend:
- Rust
- SQLite
- rusqlite or sqlx
- notify crate for file watching
- tokio for async tasks
- serde / serde_json
- chrono or time
- tracing / tracing-subscriber

Frontend:
- React
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui
- Recharts or ECharts
- Zustand or TanStack Query

Tauri plugins:
- tauri-plugin-fs
- tauri-plugin-dialog
- tauri-plugin-shell
- tauri-plugin-opener
- tauri-plugin-autostart, optional
- tauri-plugin-updater, optional
- tauri-plugin-store, optional

Tauri supports bringing your existing frontend stack, so React/Tailwind/shadcn fits well.

Tauri’s file-system plugin can read app-selected folders and file paths, which matters for importing logs/session files.

Sidecars are optional. If you ever wanted a separate Go/Node/Python collector, Tauri can bundle and run external binaries as sidecars.

3. Core architecture
Sources
├─ OpenCode logs
├─ Optional OpenCode plugin events
├─ Veyra request/response logs
├─ LM Studio local API logs/proxy
├─ OpenAI-compatible request logs
└─ Manual imports

        ↓

Collectors
├─ Log file scanner
├─ Live file watcher
├─ OpenCode plugin collector
├─ API/proxy collector
├─ Import parser
└─ Deduplication engine

        ↓

Normalizer
├─ Detect provider
├─ Detect model
├─ Detect session/project
├─ Extract exact usage fields
├─ Estimate missing tokens
├─ Calculate costs
└─ Mark confidence level

        ↓

SQLite Database
├─ events
├─ usage
├─ sessions
├─ projects
├─ models
├─ pricing
├─ sources
├─ alerts
└─ settings

        ↓

Tauri UI
├─ Overview dashboard
├─ Sessions page
├─ Models page
├─ Costs page
├─ Projects page
├─ Timeline
├─ Raw events
├─ Settings
└─ Cleanup/export tools
4. Collection modes

You should support three collection modes.

Mode A: Passive import

The app scans OpenCode’s local log/session folders.

Pros:
- Safest
- No OpenCode plugin needed
- No chance of breaking OpenCode
- Zero extra AI tokens

Cons:
- Accuracy depends on what the logs contain
- May need estimation
- May not update instantly

OpenCode’s troubleshooting docs say logs are written under ~/.local/share/opencode/log/ on macOS/Linux and %USERPROFILE%\.local\share\opencode\log on Windows.

Mode B: Live watcher

The app watches folders and imports new changes automatically.

Pros:
- Feels live
- Still no OpenCode plugin needed
- Good MVP+

Cons:
- File formats can change
- Need deduplication
Mode C: Optional OpenCode plugin

A tiny plugin listens for events and writes clean usage events to your app/database.

Pros:
- Best OpenCode integration
- Cleaner data
- Real-time updates
- Easier session/project matching

Cons:
- Needs TypeScript plugin shim
- Must be very lightweight
- OpenCode plugin API changes could affect it

OpenCode’s plugin docs describe plugins as JavaScript/TypeScript files that can hook into events and customize behavior.

5. Important rule: no extra AI tokens

The tracker uses no extra AI tokens if it only does this:

Read logs
Watch files
Parse JSON
Save SQLite rows
Render graphs
Estimate tokens locally
Calculate costs locally

It only uses extra tokens if you add AI features like:

"Summarize this session"
"Ask AI why usage was high"
"Generate optimization advice"
"Send usage report into OpenCode chat"

For a clean version, keep all analytics local.

6. Feature-complete dashboard
Overview page

Show cards:

Tokens today
Tokens this week
Tokens this month
Estimated cost today
Estimated cost month
Most used model
Most expensive model
Largest session
Average tokens per session
Input/output ratio
Reasoning token percentage
Cache savings, if available

Graphs:

Token usage over time
Cost over time
Input vs output tokens
Usage by model
Usage by provider
Usage by project
Usage by source app
Sessions page
Session list
Search by project/name/model/date
Sort by total tokens/cost/date
View session timeline
Show input/output/reasoning/cache tokens
Show exact vs estimated
Show tool call usage
Show agent/subagent usage if detectable
Session detail page
Timeline of messages/events
Tokens per turn
Cumulative tokens graph
Tool calls
Model switches
Cost estimate
Peak context size
Estimated wasted context
Raw event view
Models page
Usage by model
Cost by model
Average output tokens per model
Most expensive sessions by model
Pricing config per model
Exact/estimated accuracy rate
Projects page
Usage by project folder
Usage by Git repo
Usage by OpenCode workspace
Usage by Veyra project
Cost per project
Largest sessions per project
Costs page
Daily cost
Weekly cost
Monthly cost
Cost by provider
Cost by model
Cost forecast
Budget warnings
Custom price table
Raw events page
Search raw collected events
Filter by source/model/session/provider
View JSON payload
Copy event
Delete event
Mark as ignored
Settings page
Data sources
Watched folders
Import schedule
Storage limits
Pricing table
Token estimation mode
Privacy settings
Auto-start
Theme
Export/import database
Backup settings
7. Data to track

Track all of this:

Basic:
- timestamp
- source app
- provider
- model
- project
- session id
- message id
- request id

Token usage:
- input_tokens
- output_tokens
- total_tokens
- reasoning_tokens
- cache_read_tokens
- cache_write_tokens
- tool_tokens
- estimated_tokens
- exact_tokens

Cost:
- input_cost_usd
- output_cost_usd
- reasoning_cost_usd
- cache_read_cost_usd
- cache_write_cost_usd
- total_cost_usd

Context:
- context_window
- context_used
- context_percent
- peak_context_tokens
- estimated_remaining_context

Metadata:
- collector version
- raw source path
- raw event hash
- import status
- confidence level

Use labels like:

Exact
Estimated
Mixed
Unknown

That prevents fake precision.

8. SQLite schema

A solid starting schema:

CREATE TABLE sources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

CREATE TABLE projects (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT,
  path TEXT,
  git_remote TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(path)
);

CREATE TABLE sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_session_id TEXT NOT NULL,
  source_id INTEGER,
  project_id INTEGER,
  title TEXT,
  started_at TEXT,
  last_seen_at TEXT,
  provider TEXT,
  model TEXT,
  total_tokens INTEGER DEFAULT 0,
  total_cost_usd REAL DEFAULT 0,
  exactness TEXT DEFAULT 'unknown',
  raw_ref TEXT,
  UNIQUE(source_id, source_session_id)
);

CREATE TABLE usage_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_hash TEXT NOT NULL UNIQUE,
  timestamp TEXT NOT NULL,
  source_id INTEGER,
  session_id INTEGER,
  project_id INTEGER,
  event_type TEXT NOT NULL,
  provider TEXT,
  model TEXT,
  message_role TEXT,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  reasoning_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  tool_tokens INTEGER DEFAULT 0,
  total_tokens INTEGER DEFAULT 0,
  cost_usd REAL DEFAULT 0,
  exactness TEXT DEFAULT 'unknown',
  confidence REAL DEFAULT 0,
  raw_json TEXT
);

CREATE TABLE model_pricing (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_price_per_million REAL DEFAULT 0,
  output_price_per_million REAL DEFAULT 0,
  reasoning_price_per_million REAL DEFAULT 0,
  cache_read_price_per_million REAL DEFAULT 0,
  cache_write_price_per_million REAL DEFAULT 0,
  currency TEXT DEFAULT 'USD',
  updated_at TEXT NOT NULL,
  UNIQUE(provider, model)
);

CREATE TABLE daily_usage (
  date TEXT NOT NULL,
  provider TEXT,
  model TEXT,
  project_id INTEGER,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  reasoning_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  total_tokens INTEGER DEFAULT 0,
  cost_usd REAL DEFAULT 0,
  PRIMARY KEY(date, provider, model, project_id)
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

For performance, add indexes:

CREATE INDEX idx_usage_timestamp ON usage_events(timestamp);
CREATE INDEX idx_usage_session ON usage_events(session_id);
CREATE INDEX idx_usage_model ON usage_events(model);
CREATE INDEX idx_usage_project ON usage_events(project_id);
CREATE INDEX idx_usage_provider ON usage_events(provider);
CREATE INDEX idx_sessions_last_seen ON sessions(last_seen_at);
9. Storage strategy

Do not store full conversations by default.

Use levels:

Minimal mode:
- only totals
- cheapest storage
- best privacy

Detailed mode:
- per message/event token rows
- good graphs
- still small

Debug mode:
- raw JSON events
- useful while developing

Archive mode:
- full text/tool outputs
- optional only
- can get huge

Recommended default:

Store forever:
- totals
- token fields
- model/provider/session/project/date
- event hashes
- cost estimates

Store for 7–30 days:
- raw JSON

Do not store by default:
- full prompts
- full responses
- terminal outputs
- code diffs
- environment variables
- secrets

Estimated storage:

Light mode:
1–5 MB/month

Detailed mode:
10–100 MB/month

Debug/raw mode:
100 MB–2+ GB/month depending on logs and tool output
10. Token estimation

You need two paths:

Exact usage

Use exact token fields when available from the source.

Best accuracy:
- provider reports usage
- OpenCode exposes usage
- Veyra logs usage from API responses
- local proxy captures API responses
Estimated usage

When exact usage is unavailable:

Estimate from text
Approximate per model family
Mark result as estimated
Do not pretend cost is exact

Possible approaches:

Fast estimate:
characters / 4

Better estimate:
model-specific tokenizer

Best local estimate:
provider-compatible tokenizer crates/libraries

For the first version, characters / 4 is enough for trend graphs, but not exact billing.

11. Pricing engine

Pricing must be editable because model prices change.

Fields:

provider
model
input price per 1M tokens
output price per 1M tokens
reasoning price per 1M tokens
cache read price per 1M tokens
cache write price per 1M tokens
currency
effective date
source/manual flag

Features:

Custom model pricing
Local model marked as $0
LM Studio local usage marked as local/free
Historical pricing snapshots
Cost recalculation button
Warning if pricing missing

Do not hardcode everything permanently.

12. Background behavior

The app can run in three ways:

Manual:
User opens app and imports data.

Background while app open:
File watcher updates dashboard.

System tray:
App keeps collecting while minimized.

Optional auto-start:
Starts collector on Windows login.

For Tauri, the safest setup is:

Default:
No background auto-start.

Optional setting:
"Start TokenLens when Windows starts."

Optional tray:
Minimize to tray and keep watching.
13. Optional OpenCode collector plugin

Even if the main app is Tauri/Rust, the OpenCode plugin itself should stay tiny:

OpenCode TypeScript plugin
↓
Receives event
↓
Writes small JSONL file or calls local HTTP endpoint
↓
Tauri app imports it

Do not make the plugin do heavy work.

Bad plugin behavior:

Running expensive tokenizers
Scanning huge files
Writing giant raw logs
Calling AI
Blocking OpenCode events
Doing database migrations

Good plugin behavior:

Listen
Normalize lightly
Write event
Return immediately

Possible plugin output:

{
  "timestamp": "2026-06-14T13:00:00Z",
  "source": "opencode",
  "event_type": "message.updated",
  "session_id": "abc123",
  "project_path": "C:/Users/isaac/project",
  "provider": "openai",
  "model": "gpt-5.5",
  "usage": {
    "input_tokens": 1000,
    "output_tokens": 500,
    "reasoning_tokens": 0,
    "cache_read_tokens": 0,
    "cache_write_tokens": 0
  },
  "exactness": "exact"
}
14. Local API option

For the best app experience, the Tauri backend can expose a local collector endpoint:

http://127.0.0.1:43177/ingest

The plugin or other tools send JSON events to it.

Security rules:

Bind only to 127.0.0.1
Require local auth token
Rotate token
Reject huge payloads
Rate-limit requests
Validate schema
Never expose to LAN by default

Alternative: write JSONL files instead of HTTP.

%APPDATA%/TokenLens/inbox/opencode.jsonl

JSONL is simpler and safer for MVP.

15. Privacy and safety

This matters because logs can contain code, prompts, secrets, and terminal output.

Required privacy features:

Local-only by default
No telemetry by default
No cloud sync by default
No AI calls by default
Do not store full prompt/response text by default
Redact obvious secrets from raw events
Allow raw event storage to be disabled
Allow full database delete
Allow per-project ignore
Allow path anonymization
Allow export without raw JSON

Secret redaction patterns:

API keys
.env values
Bearer tokens
GitHub tokens
OpenAI/Anthropic/Gemini keys
AWS keys
Private SSH keys
JWTs
Database URLs
16. UI design

Since this is a data app, keep the UI clean.

Sidebar
Overview
Sessions
Projects
Models
Providers
Costs
Timeline
Raw Events
Settings
Top filters
Date range
Source app
Project
Provider
Model
Exactness
Main cards
Tokens
Cost
Sessions
Largest spike
Top model
Top project
Charts

Use:

Line chart:
tokens/cost over time

Stacked bar:
input vs output vs reasoning

Pie/donut:
usage share by model/provider

Table:
sessions and raw rows

Heatmap:
usage by hour/day, optional
17. Notifications and alerts

Feature-complete version should include:

Daily token limit warning
Monthly cost warning
Huge session warning
Context window almost full warning
Model unusually expensive warning
Raw logs growing too large warning
Collector disconnected warning
Pricing missing warning

Example:

"Project Veyra used 1.2M tokens today."
"OpenCode collector has not sent data in 2 hours."
"Raw event storage is using 850 MB."
18. Exports

Support:

CSV export
JSON export
SQLite backup
Monthly report
Project report
Model report
Cost report

Optional later:

PDF report
Markdown report
PNG chart export
19. Cleanup tools

Add storage controls:

Delete raw events older than 30 days
Keep aggregates forever
Vacuum database
Recalculate costs
Rebuild daily aggregates
Remove ignored projects
Reset all data
20. Settings needed
General:
- theme
- start on boot
- minimize to tray
- default date range

Sources:
- OpenCode folder path
- Veyra database/log path
- LM Studio logs/proxy setting
- custom folders

Privacy:
- store raw JSON yes/no
- store message text yes/no
- redact secrets yes/no
- anonymize paths yes/no

Costs:
- currency
- pricing table
- local models cost $0
- missing price behavior

Storage:
- raw retention days
- max database size
- auto-cleanup

Advanced:
- import interval
- watcher enabled
- debug logs
- collector endpoint enabled
21. Project folder structure
tokenlens/
├─ package.json
├─ src/
│  ├─ app/
│  ├─ components/
│  ├─ pages/
│  ├─ charts/
│  ├─ stores/
│  ├─ lib/
│  └─ types/
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ commands/
│  │  ├─ db/
│  │  ├─ collectors/
│  │  │  ├─ opencode.rs
│  │  │  ├─ veyra.rs
│  │  │  ├─ lmstudio.rs
│  │  │  └─ jsonl.rs
│  │  ├─ ingest/
│  │  ├─ pricing/
│  │  ├─ token_estimator/
│  │  ├─ redaction/
│  │  ├─ aggregation/
│  │  ├─ settings/
│  │  └─ errors.rs
│  └─ migrations/
├─ collectors/
│  └─ opencode-plugin/
│     └─ tokenlens.ts
└─ docs/
   ├─ architecture.md
   ├─ data-model.md
   └─ privacy.md
22. Tauri commands

Frontend should call Rust commands like:

get_overview_stats(range)
get_usage_timeseries(range, filters)
get_sessions(filters)
get_session_detail(session_id)
get_model_breakdown(range, filters)
get_project_breakdown(range, filters)
get_cost_breakdown(range, filters)
get_sources()
add_source(path, kind)
scan_source(source_id)
start_watcher(source_id)
stop_watcher(source_id)
update_pricing(model_price)
recalculate_costs()
export_csv(filters)
cleanup_raw_events(days)
get_settings()
update_settings(settings)
23. Build phases
Phase 1 — MVP
Tauri app
SQLite database
Manual OpenCode log import
Overview dashboard
Sessions table
Model breakdown
Basic cost table
Estimated/exact labels

Difficulty: 5/10

Phase 2 — Live collection
Folder watcher
Auto-import
Deduplication
Settings page
Storage cleanup
Better charts

Difficulty: 6/10

Phase 3 — Optional OpenCode plugin
Tiny TypeScript plugin
JSONL or localhost ingest
Cleaner session events
Realtime updates
Collector health status

Difficulty: 6–7/10

Phase 4 — Full analytics
Per-project analytics
Per-tool usage
Cost forecasting
Budget alerts
Raw event viewer
Advanced filters
Exports

Difficulty: 7/10

Phase 5 — Multi-tool support
Veyra adapter
LM Studio adapter
OpenAI-compatible proxy
Claude Code/Cursor adapters if desired

Difficulty: 7–8/10

24. Hardest parts

The hard parts are not the graphs. The hard parts are:

1. Finding reliable token data in each source
2. Handling changing OpenCode log formats
3. Deduplicating events
4. Separating exact vs estimated usage
5. Keeping storage small
6. Avoiding storing sensitive content
7. Making the collector not slow down OpenCode
25. My recommended final design

For you, I would build:

TokenLens

Tauri v2 desktop app
React + TypeScript + Tailwind + shadcn/ui
Rust backend
SQLite database
OpenCode passive importer first
Live watcher second
Optional OpenCode plugin third
Veyra adapter fourth

Default behavior:

No AI calls
No extra OpenCode tokens
No cloud
No telemetry
No full message storage
Only local analytics
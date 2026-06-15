# Contributing

Thanks for your interest in TokenLens. The project is local-first by
design, so most contributions are about the Tauri / React / Rust stack
and the data pipeline.

## Ground rules

- **No telemetry, no cloud, no AI calls in the analytics path.** New
  features that would change this need to be opt-in and called out
  clearly in the PR.
- **Privacy first.** Don't add code that logs or persists prompts,
  responses, secrets, or full paths unless an existing setting already
  controls that.
- **Small, focused PRs.** A bug fix that touches one file is easier to
  review than a sweep across pages and the Rust backend.

## Development setup

Requirements:

- Node.js 18+
- Rust 1.77+
- Platform-specific [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)
  (WebView2 on Windows, WebKitGTK on Linux, Xcode CLT on macOS)

```bash
git clone https://github.com/<your-username>/tokenlens.git
cd tokenlens/tokenlens
npm install
npm run dev          # plain Vite — uses the in-memory mock backend
npm run tauri:dev    # full desktop app, talks to the Rust backend
```

The mock backend in `src/lib/mock.ts` lets you exercise the UI without
Tauri. It's also what the screenshots are generated against.

## Quality gates

Run these before opening a PR:

```bash
npm run lint          # ESLint
npm run typecheck     # tsc --noEmit
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

CI runs the same three jobs on every push and PR. See
`.github/workflows/ci.yml`.

## Project layout

- `src/` — React + TypeScript frontend
- `src-tauri/src/` — Rust backend (commands, db, ingest, collectors,
  pricing, redaction, watcher, settings)
- `src-tauri/migrations/` — SQL migrations
- `collectors/opencode-plugin/` — small TypeScript OpenCode plugin
- `docs/` — architecture, data model, privacy, design notes

## Adding a new collector

1. Implement the parser under `src-tauri/src/ingest/` and add a
   `normalize_event` entry that produces a canonical `UsageEvent`.
2. Wire it up in the `ingest` command surface in
   `src-tauri/src/commands/`.
3. Update the mock backend in `src/lib/mock.ts` so the UI can be
   exercised without Tauri.
4. Add a fixture in `src-tauri/tests/fixtures/` and at least one
   parser test in `src-tauri/tests/`.
5. Document the source mode in `tokenlens/README.md`.

## Adding a new page

1. Add the route in `src/App.tsx`.
2. Add a sidebar entry in `src/components/layout/Sidebar.tsx`.
3. Reuse primitives from `src/components/ui/primitives.tsx`.
4. If you add charts, extend `src/charts/index.tsx` rather than
   importing Recharts directly.

## Screenshots

If your change is user-visible, please attach before/after screenshots
to the PR. The repo currently uses dark-mode captures; a quick way to
generate one is to run the app, hit the relevant route, and use your
platform's screenshot tool. (Puppeteer/Edge headless works too — see
the capture notes in `docs/screenshots/`.)

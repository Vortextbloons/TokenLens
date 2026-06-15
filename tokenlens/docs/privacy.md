# TokenLens — Privacy

## Principles

1. **No network calls.** The Tauri process has no HTTP client. No analytics, no
   update checks, no crash reporting.
2. **No cloud sync.** Your data never leaves the device.
3. **No AI calls.** Analytics are pure SQL + arithmetic. The OpenCode plugin
   doesn't call any model.
4. **Minimal data by default.** Token counts and metadata, not full prompts.

## What is stored

### Always

- Token counts (input, output, reasoning, cache read/write, total)
- Provider, model, session id, project id
- Timestamp, event type, exactness label
- Cost estimate (derived from your pricing table)
- Event hash (SHA-256 of canonicalized fields) for dedup

### 7–30 days (configurable; default 14)

- Raw JSON of the event, **after** secret redaction

### Off by default (and we do not collect it)

- Full message text (prompts and completions)
- Terminal output
- Code diffs
- Environment variables
- Secret values (stripped even when raw JSON is on)

## Secret redaction

TokenLens runs a regex-based redactor over raw JSON before it hits disk.
The redactor covers:

- OpenAI (`sk-...`, `sk-proj-...`)
- Anthropic (`sk-ant-...`)
- Google (`AIza...`)
- GitHub PATs, OAuth, user, server, refresh, fine-grained
- AWS access key / secret key
- Slack tokens
- Stripe live/test keys
- HuggingFace tokens
- JWTs
- PEM private key blocks
- `Bearer …` headers
- Common `.env`-style `KEY=value` pairs

The redactor replaces matches with `[REDACTED:kind]` placeholders. A unit
test suite in `src-tauri/src/redaction/mod.rs` covers each pattern.

## Settings that affect privacy

| Setting | Default | Effect |
| --- | --- | --- |
| Store raw JSON | ON | Raw event JSON kept for 7–30 days |
| Store full message text | OFF | Not implemented in v1; reserved |
| Redact obvious secrets | ON | Redaction runs before storage |
| Anonymize filesystem paths | OFF | Replaces path strings in stored data with hashed tokens |
| Autostart on login | OFF | Doesn't change what we store |
| Debug logging | OFF | Verbose logs to `<app data>/TokenLens/logs/` |

## What we never do

- Send your data anywhere.
- Read the source folders you point us at beyond parsing JSON/JSONL/log files.
- Touch your OpenCode or Veyra configuration.
- Make outgoing network requests.

## Reset

**Settings → Storage → Reset all data** deletes all events, sessions,
projects, alerts, and pricing history. The DB is preserved; you can start
collecting again immediately. To wipe the DB file, delete
`<app local data>/TokenLens/tokenlens.sqlite` from your filesystem.

## Threat model

TokenLens is designed to operate on a single user's machine, with the user as
the principal. It is not designed to defend against a malicious local actor
who already has the ability to read your app data folder. If an attacker has
that access, your data is at risk regardless of what TokenLens does.

We do defend against:

- **Secrets leaking into raw JSON storage** (redaction).
- **Accidental network egress** (no HTTP client compiled in).
- **Accidental writes to source folders** (collector is read-only by design).
- **Runaway storage growth** (retention settings + auto-cleanup).

# TokenLens OpenCode Plugin

A tiny OpenCode plugin that pipes token usage to **TokenLens** via a JSONL inbox file. It does **no** tokenization, **no** network calls, and never blocks the OpenCode main thread.

## Install

Copy `tokenlens.ts` into your OpenCode project's `plugins/` directory. OpenCode auto-discovers it.

## Configure (optional)

| Env var | Description | Default |
| --- | --- | --- |
| `TOKENLENS_INBOX` | Full path to the JSONL inbox file | `<app data>/TokenLens/inbox/opencode.jsonl` |
| `TOKENLENS_DEBUG` | Print errors to stderr | unset |

## Event format

Each event is one JSON line appended to the inbox. TokenLens scans the inbox and persists it.

```json
{
  "timestamp": "2026-06-14T13:00:00Z",
  "source": "opencode",
  "plugin_version": "0.1.0",
  "event_type": "message",
  "session_id": "abc123",
  "project_path": "C:/Users/you/project",
  "provider": "openai",
  "model": "gpt-4o",
  "usage": {
    "input_tokens": 1000,
    "output_tokens": 500,
    "reasoning_tokens": 0,
    "cache_read_tokens": 0,
    "cache_write_tokens": 0
  },
  "total_tokens": 1500,
  "exactness": "exact"
}
```

## What it does NOT do

- No API calls to any provider.
- No tokenization (TokenLens handles that locally if needed).
- No DB writes — TokenLens owns the database.
- No heavy computation in event handlers.

## Privacy

The plugin does not modify OpenCode's behavior beyond appending a single small line per relevant event. Disable by removing the file from `plugins/`.

/* eslint-disable */
// TokenLens — optional OpenCode plugin (Phase 3).
//
// This file lives next to your opencode project and is picked up by the
// OpenCode plugin system. It listens for message and step events, then
// appends a small JSONL line to the TokenLens inbox folder. TokenLens
// scans the inbox in the background and persists events.
//
// Constraints we honor:
//   * No expensive work in the event handler (no tokenization, no DB).
//   * No blocking I/O on the OpenCode main thread.
//   * No calls to OpenAI / Anthropic / etc.
//   * Writes at most a few hundred bytes per event.
//
// To install: copy this file into your opencode plugins directory and
// (optionally) edit INBOX_PATH below.

import { appendFile } from "node:fs/promises";
import { homedir, platform } from "node:os";
import { join } from "node:path";

const PLATFORM = platform();
const HOME = homedir();

function defaultInboxPath() {
  if (PLATFORM === "win32") {
    return join(
      process.env.APPDATA || join(HOME, "AppData", "Roaming"),
      "TokenLens",
      "inbox",
      "opencode.jsonl",
    );
  }
  if (PLATFORM === "darwin") {
    return join(HOME, "Library", "Application Support", "TokenLens", "inbox", "opencode.jsonl");
  }
  return join(HOME, ".local", "share", "TokenLens", "inbox", "opencode.jsonl");
}

const INBOX_PATH = process.env.TOKENLENS_INBOX || defaultInboxPath();
const SESSION_ID = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

function pickSessionId(payload) {
  return (
    payload?.sessionID ||
    payload?.sessionId ||
    payload?.session_id ||
    payload?.conversation_id ||
    SESSION_ID
  );
}

function pickModel(payload) {
  return (
    payload?.modelID ||
    payload?.model_id ||
    payload?.model ||
    payload?.info?.modelID ||
    payload?.info?.model ||
    null
  );
}

function pickProvider(payload, model) {
  return (
    payload?.providerID ||
    payload?.provider_id ||
    payload?.provider ||
    payload?.info?.providerID ||
    payload?.info?.provider ||
    (model ? detectProviderFromModel(model) : null)
  );
}

function detectProviderFromModel(m) {
  const s = String(m).toLowerCase();
  if (s.startsWith("gpt-") || s.startsWith("o1") || s.startsWith("o3") || s.startsWith("o4")) return "openai";
  if (s.startsWith("claude-")) return "anthropic";
  if (s.startsWith("gemini-") || s.startsWith("palm-")) return "google";
  if (s.includes("llama") || s.includes("qwen") || s.includes("mistral") || s.includes("phi-")) return "local";
  if (s.includes("lm-studio")) return "lmstudio";
  return "unknown";
}

function asInt(x) {
  if (typeof x === "number") return Math.trunc(x);
  if (typeof x === "string") {
    const n = Number(x);
    if (Number.isFinite(n)) return Math.trunc(n);
  }
  return 0;
}

function extractUsage(payload) {
  const usage = payload?.usage || payload?.tokens || payload?.info?.tokens || payload?.info?.usage;
  if (!usage) return null;
  const input = asInt(usage.input_tokens ?? usage.prompt_tokens ?? usage.input ?? usage.inputTokens);
  const output = asInt(usage.output_tokens ?? usage.completion_tokens ?? usage.output ?? usage.outputTokens);
  const total = asInt(usage.total_tokens ?? usage.total ?? (input + output));
  const reasoning = asInt(
    usage.reasoning_tokens ??
      usage.reasoningTokens ??
      usage.completion_tokens_details?.reasoning_tokens ??
      usage.output_tokens_details?.reasoning_tokens ??
      0,
  );
  const cache_read = asInt(
    usage.cache_read_input_tokens ??
      usage.cache_read_tokens ??
      usage.cacheReadTokens ??
      usage.prompt_tokens_details?.cached_tokens ??
      0,
  );
  const cache_write = asInt(
    usage.cache_creation_input_tokens ??
      usage.cache_write_tokens ??
      usage.cacheWriteTokens ??
      0,
  );
  return { input, output, reasoning, cache_read, cache_write, total };
}

async function writeEvent(event) {
  const line = JSON.stringify({ ...event, source: "opencode", plugin_version: "0.1.0" }) + "\n";
  try {
    await appendFile(INBOX_PATH, line, "utf8");
  } catch (e) {
    // Best effort: never crash OpenCode over a plugin error.
    if (process.env.TOKENLENS_DEBUG) {
      console.error("[tokenlens] write failed:", e?.message);
    }
  }
}

function handleMessage(eventType) {
  return async ({ payload }) => {
    const model = pickModel(payload);
    if (!model) return; // ignore events with no model signal
    const provider = pickProvider(payload, model);
    const usage = extractUsage(payload) || { input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0, total: 0 };
    if (!usage.total) return;
    await writeEvent({
      timestamp: new Date().toISOString(),
      event_type: eventType,
      session_id: pickSessionId(payload),
      project_path: payload?.cwd || payload?.workspace || payload?.project_path || null,
      provider,
      model,
      usage: {
        input_tokens: usage.input,
        output_tokens: usage.output,
        reasoning_tokens: usage.reasoning,
        cache_read_tokens: usage.cache_read,
        cache_write_tokens: usage.cache_write,
      },
      total_tokens: usage.total,
      exactness: "exact",
    });
  };
}

export const TokenLensPlugin = async () => ({
  event: async ({ event }) => {
    try {
      const t = event?.type || event?.event;
      if (t === "message.updated" || t === "message.complete" || t === "message.finished") {
        await handleMessage("message")(event);
      } else if (t === "step.finish" || t === "step.complete" || t === "step.finished") {
        await handleMessage("step")(event);
      } else if (t === "session.start" || t === "session.created") {
        await writeEvent({
          timestamp: new Date().toISOString(),
          event_type: "session_start",
          session_id: pickSessionId(event?.payload ?? event),
          project_path: event?.payload?.cwd || null,
          provider: null,
          model: null,
          usage: { input_tokens: 0, output_tokens: 0, reasoning_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 },
          total_tokens: 0,
          exactness: "exact",
        });
      }
    } catch (e) {
      if (process.env.TOKENLENS_DEBUG) {
        console.error("[tokenlens] event handler failed:", e?.message);
      }
    }
  },
});

export default TokenLensPlugin;

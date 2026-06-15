// cn() — combines clsx + tailwind-merge for safe className composition.
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNumber(n: number): string {
  if (!isFinite(n)) return "0";
  if (Math.abs(n) >= 1_000_000) return (n / 1_000_000).toFixed(2) + "M";
  if (Math.abs(n) >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return Math.round(n).toLocaleString();
}

export function formatFull(n: number): string {
  return Math.round(n).toLocaleString();
}

export function formatUsd(n: number): string {
  if (!isFinite(n)) return "$0.00";
  if (Math.abs(n) < 0.01 && n !== 0) return "<$0.01";
  return "$" + n.toFixed(2);
}

export function formatPercent(n: number): string {
  return n.toFixed(1) + "%";
}

export interface DeltaValue {
  /** Human-readable label, e.g. "+12.3%" or "-100%" or "new". */
  value: string;
  /** True = render in the "good" color (green for the current default). */
  positive: boolean;
}

/**
 * Period-over-period delta for a single KPI.
 *
 * - Both zero  -> null (no card chip).
 * - Previous 0, current > 0 -> "new" (positive=false per default convention).
 * - Current 0, previous > 0 -> "-100%".
 * - Otherwise: signed percent change. `positive` follows the
 *   "up = good = green" convention by default (matches the rest of the UI).
 */
export function formatDelta(
  current: number,
  previous: number,
  pct = (n: number) => `${n.toFixed(1)}%`,
): DeltaValue | null {
  if (current === 0 && previous === 0) return null;
  if (previous === 0) return { value: "new", positive: false };
  if (current === 0) return { value: "-100%", positive: false };
  const change = ((current - previous) / previous) * 100;
  const positive = change >= 0;
  const label = `${positive ? "+" : ""}${pct(change)}`;
  return { value: label, positive };
}

export function formatDate(d: string | null | undefined, withTime = true): string {
  if (!d) return "—";
  const date = new Date(d);
  if (isNaN(date.getTime())) return d;
  const datePart = date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  if (!withTime) return datePart;
  const timePart = date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return `${datePart} ${timePart}`;
}

export function localDateString(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Inclusive calendar-day ranges in local time (7d = today + prior 6 days). */
export function rangeToDates(range: string): { start: string | null; end: string | null } {
  const end = new Date();
  const start = new Date();
  switch (range) {
    case "1d":
      break;
    case "7d":
      start.setDate(end.getDate() - 6);
      break;
    case "30d":
      start.setDate(end.getDate() - 29);
      break;
    case "90d":
      start.setDate(end.getDate() - 89);
      break;
    case "all":
      return { start: null, end: null };
    default:
      start.setDate(end.getDate() - 6);
  }
  return {
    start: localDateString(start),
    end: localDateString(end),
  };
}

export function exactnessColor(e: string): string {
  switch (e) {
    case "exact": return "text-emerald-500";
    case "estimated": return "text-amber-500";
    case "mixed": return "text-blue-500";
    default: return "text-zinc-500";
  }
}

/** Human-readable label for token/cost confidence. */
export function formatExactness(e: string): string {
  switch (e) {
    case "exact": return "Exact";
    case "estimated": return "Est. cost";
    case "mixed": return "Exact tokens";
    case "unknown": return "Unknown";
    default: return e;
  }
}

/**
 * Best-effort human-readable string for any thrown value.
 *
 * Tauri commands return errors as a structured `{ kind, message }` object
 * (see src-tauri/src/errors.rs). `String(err)` would render that as
 * "[object Object]" in the UI, so this helper unwraps common shapes and
 * falls back to JSON serialization. Never throws.
 */
export function errorMessage(e: unknown): string {
  if (e == null) return String(e);
  if (typeof e === "string") return e;
  if (typeof e === "number" || typeof e === "boolean" || typeof e === "bigint") {
    return String(e);
  }
  if (e instanceof Error) {
    return e.message || e.name || "Error";
  }
  if (typeof e === "object") {
    const obj = e as Record<string, unknown>;
    const message = obj.message;
    const kind = obj.kind;
    if (typeof message === "string" && typeof kind === "string") {
      return `${kind}: ${message}`;
    }
    if (typeof message === "string") return message;
    if (typeof kind === "string" && typeof obj.error === "string") {
      return `${kind}: ${obj.error}`;
    }
    try {
      return JSON.stringify(e);
    } catch {
      return Object.prototype.toString.call(e);
    }
  }
  return String(e);
}

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

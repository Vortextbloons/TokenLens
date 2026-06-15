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
  return "$" + n.toFixed(n < 1 ? 4 : 2);
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

export function rangeToDates(range: string): { start: string | null; end: string | null } {
  const end = new Date();
  const start = new Date();
  switch (range) {
    case "1d": start.setDate(end.getDate() - 1); break;
    case "7d": start.setDate(end.getDate() - 7); break;
    case "30d": start.setDate(end.getDate() - 30); break;
    case "90d": start.setDate(end.getDate() - 90); break;
    case "all": return { start: null, end: null };
    default: start.setDate(end.getDate() - 7);
  }
  return {
    start: start.toISOString().slice(0, 10),
    end: end.toISOString().slice(0, 10),
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

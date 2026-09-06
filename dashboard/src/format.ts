import type { Health } from "./schema";
export const healthText: Record<Health, string> = { healthy: "Healthy", degraded: "Degraded", unhealthy: "Unhealthy", unknown: "Unknown", expected_inactive: "Expected inactive" };
export function age(at: string | null, now: number): string {
  if (!at) return "Awaiting observation";
  const seconds = Math.max(0, Math.floor((now - Date.parse(at)) / 1000));
  if (!Number.isFinite(seconds)) return "Unknown age";
  if (seconds < 5) return "Just now";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}
export function stale(at: string | null, now: number): boolean { return at !== null && now > Date.parse(at); }
export function rule(text: string): string { return text.replaceAll("-", " ").replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase()); }
export function utc(at: string): string { return new Date(at).toISOString().replace("T", " ").replace(/\.\d+Z$/, " UTC"); }

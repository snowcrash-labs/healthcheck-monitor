import type { z } from "zod";

export type Result<T> = { ok: true; value: T } | { ok: false; message: string; cancelled: boolean; unauthorized?: boolean; refreshRequired?: boolean };
export async function get<T>(path: string, schema: z.ZodType<T>, signal: AbortSignal): Promise<Result<T>> {
  try {
    const response = await fetch(path, { signal: AbortSignal.any([signal, AbortSignal.timeout(10_000)]), credentials: "same-origin", cache: "no-store" });
    if (!response.ok) {
      const unauthorized = response.status === 401 || response.status === 403;
      if (unauthorized) window.dispatchEvent(new Event("monitor:unauthorized"));
      return { ok: false, cancelled: false, unauthorized, refreshRequired: response.status === 409, message: unauthorized ? "Your sign-in has expired or access was removed. Sign in again to continue." : response.status === 409 ? "The published view changed. Refresh this selection." : response.status === 404 ? "This observation is no longer available." : "Monitoring data is temporarily unavailable." };
    }
    const raw: unknown = await response.json();
    const parsed = schema.safeParse(raw);
    return parsed.success ? { ok: true, value: parsed.data } : { ok: false, cancelled: false, message: "The dashboard received an invalid update." };
  } catch {
    return { ok: false, cancelled: signal.aborted, message: "The dashboard could not reach the monitoring service." };
  }
}
export function query(path: string, values: Record<string, string | undefined>): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) if (value) params.set(key, value);
  return `${path}${params.size ? `?${params.toString()}` : ""}`;
}
/** Opaque route segments preserve slashes and literal percent escapes through router decoding. */
export function resourcePath(id: string, target?: string): string {
  let binary = "";
  for (const byte of new TextEncoder().encode(id)) binary += String.fromCharCode(byte);
  return query(`/resources/${btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "")}`, { target });
}
export function resourceId(segment: string | undefined): string | undefined {
  if (!segment || segment.length > 8192 || !/^[A-Za-z0-9_-]+$/.test(segment)) return undefined;
  try {
    const decoded = atob(segment.replaceAll("-", "+").replaceAll("_", "/"));
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(decoded, (character) => character.charCodeAt(0)));
  } catch { return undefined; }
}

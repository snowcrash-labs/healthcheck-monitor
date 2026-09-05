import { z } from "zod";

export const health = z.enum(["healthy", "degraded", "unhealthy", "unknown", "expected_inactive"]);
export const severity = z.enum(["info", "warning", "error"]);
export const expected = z.enum(["active", "dormant", "scale_to_zero", "suspended"]);
export const confidence = z.enum(["direct", "correlated", "insufficient"]);
export const check = z.enum(["preflight", "discovery", "inventory", "kubernetes", "edge", "managed", "queues", "releases", "github", "metrics", "logs", "alerts", "slo", "flows"]);
const timestamp = z.iso.datetime({ offset: true });
const count = z.number().int().nonnegative();
export const finding = z.object({
  id: z.string(), target: z.string(), resource: z.string(), rule: z.string(), severity,
  observed_at: timestamp, valid_until: timestamp.nullable(), expected, confidence,
  stale: z.boolean(), evidence: z.array(z.string()),
});
export const resource = z.object({
  id: z.string(), target: z.string(), check, health, expected,
  observed_at: timestamp, expires_at: timestamp,
  facts: z.array(z.object({ label: z.string(), value: z.string() })),
});
const checkView = z.object({
  key: z.string(), target: z.string(), check, interval_seconds: count,
  finished_at: timestamp.nullable(), expires_at: timestamp.nullable(), complete: z.boolean(), observations: count,
  failures: z.array(z.object({ operation: z.string(), coverage: z.string(), required: z.boolean() })),
});
export const overview = z.object({
  generation: count, configuration_revision: z.string(), captured_at: timestamp,
  heartbeat_at: timestamp.nullable(), running: z.boolean(), persistence_fault: z.boolean(), view_truncated: z.boolean(),
  history: z.object({ available: z.boolean(), last_persisted_at: timestamp.nullable(), dropped_events: count, dropped_runs: count, queued_batches: count, gaps: count }),
  totals: z.object({ targets: count, resources: count, error_findings: count, warning_findings: count, incomplete_checks: count }),
  targets: z.array(z.object({ name: z.string(), provider: z.string(), scope: z.string(), regions: z.array(z.string()), health, complete_checks: count, total_checks: count, errors: count, warnings: count, resources: count, latest_observation: timestamp.nullable() })),
  checks: z.array(checkView),
});
export const findingPage = z.object({ generation: count, items: z.array(finding), next_cursor: z.string().nullable(), total: count });
export const resourcePage = z.object({ generation: count, items: z.array(resource), next_cursor: z.string().nullable(), total: count });
export const resourceDetail = z.object({ generation: count, resource, findings: z.array(finding) });
export const historyEvent = z.object({
  id: z.uuidv7(), configuration_id: z.uuidv7(), target: z.string(), finding: z.string(), resource: z.string(), rule: z.string(),
  kind: z.enum(["new", "worsened", "recovered", "stale", "removed", "reappeared"]), severity, expected, confidence,
  observed_at: timestamp, at: timestamp, stale: z.boolean(), evidence: z.array(z.string()),
});
export const historyPage = z.object({ items: z.array(historyEvent), next_cursor: z.string().nullable(), gaps: count });
export type Overview = z.infer<typeof overview>;
export type Finding = z.infer<typeof finding>;
export type Resource = z.infer<typeof resource>;
export type Health = z.infer<typeof health>;
export type HistoryEvent = z.infer<typeof historyEvent>;

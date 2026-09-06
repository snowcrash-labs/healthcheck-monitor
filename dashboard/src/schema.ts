import { z } from "zod";

export const health = z.enum(["healthy", "degraded", "unhealthy", "unknown", "expected_inactive"]);
export const severity = z.enum(["info", "warning", "error"]);
export const expected = z.enum(["active", "dormant", "scale_to_zero", "suspended"]);
export const confidence = z.enum(["direct", "correlated", "insufficient"]);
export const check = z.enum(["preflight", "discovery", "inventory", "kubernetes", "edge", "managed", "queues", "releases", "github", "metrics", "logs", "alerts", "slo", "flows"]);
const timestamp = z.iso.datetime({ offset: true });
const count = z.number().int().nonnegative();
export const facts = z.array(z.object({ label: z.string(), value: z.string() }));
export const consoleLink = z.object({ label: z.string(), url: z.url().refine((value) => {
  if (!URL.canParse(value)) return false;
  const url = new URL(value);
  if (url.username || url.password || url.port) return false;
  return url.protocol === "https:" && (url.hostname === "console.cloud.google.com" || url.hostname === "portal.azure.com" || url.hostname === "console.aws.amazon.com" || url.hostname.endsWith(".console.aws.amazon.com"));
}) });
export const resourceContext = z.object({ provider: z.string(), scope: z.string(), service: z.string(), native_id: z.string(), region: z.string().nullable(), zone: z.string().nullable(), cluster: z.string().nullable(), namespace: z.string().nullable(), name: z.string().nullable(), uid: z.string().nullable(), container: z.string().nullable(), reason: z.string().nullable().default(null), exit_code: z.number().int().nullable().default(null) });
export const diagnostic = z.object({ first_detected_at: timestamp.nullable(), last_detected_at: timestamp, context: resourceContext.nullable(), facts, links: z.array(consoleLink) });
export const finding = z.object({
  check: check.nullable().optional(), diagnostic: diagnostic.nullable().optional(),
  id: z.string(), target: z.string(), resource: z.string(), rule: z.string(), severity,
  observed_at: timestamp, valid_until: timestamp.nullable(), expected, confidence,
  stale: z.boolean(), evidence: z.array(z.string()),
});
export const resource = z.object({
  checks: z.array(check).default([]), context: resourceContext.nullable().default(null), links: z.array(consoleLink).default([]),
  id: z.string(), target: z.string(), check, health, expected,
  observed_at: timestamp, expires_at: timestamp,
  facts,
});
export const coverage = z.enum(["complete", "denied", "unauthenticated", "unavailable", "unsupported", "missing", "truncated", "timeout", "cancelled", "malformed", "stale", "inventory_only"]);
export const checkView = z.object({
  started_at: timestamp.nullable(), required_failures: count, optional_gaps: count, prerequisite: z.boolean(),
  key: z.string(), target: z.string(), check, interval_seconds: count,
  finished_at: timestamp.nullable(), expires_at: timestamp.nullable(), complete: z.boolean(), observations: count,
  failures: z.array(z.object({ operation: z.string(), coverage, required: z.boolean() })),
});
export const overview = z.object({
  generation: count, configuration_revision: z.string(), captured_at: timestamp,
  heartbeat_at: timestamp.nullable(), running: z.boolean(), persistence_fault: z.boolean(),
  history: z.object({ available: z.boolean(), last_persisted_at: timestamp.nullable(), dropped_events: count, dropped_runs: count, queued_batches: count, gaps: count }),
  totals: z.object({ targets: count, resources: count, error_findings: count, warning_findings: count, incomplete_checks: count }),
  targets: z.array(z.object({ name: z.string(), provider: z.string(), scope: z.string(), regions: z.array(z.string()), health, complete_checks: count, total_checks: count, errors: count, warnings: count, resources: count, latest_observation: timestamp.nullable() })),
  checks: z.array(checkView),
});
export const findingPage = z.object({ generation: count, items: z.array(finding), next_cursor: z.string().nullable(), previous_cursor: z.string().nullable(), total: count });
const resourceRow = resource.extend({
  finding_count: count,
  findings: z.array(z.object({ diagnostic: diagnostic.nullable().optional(), rule: z.string(), severity, observed_at: timestamp, valid_until: timestamp.nullable(), stale: z.boolean() })).max(2),
});
export const resourcePage = z.object({ generation: count, items: z.array(resourceRow), next_cursor: z.string().nullable(), previous_cursor: z.string().nullable(), total: count });
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

const paged = <T extends z.ZodType>(item: T) => z.object({ generation: count, items: z.array(item), next_cursor: z.string().nullable(), previous_cursor: z.string().nullable(), total: count });
export const checkPage = paged(checkView.transform((row) => ({ ...row, id: row.key })));
export const operation = z.object({ id: z.string(), coverage, observed_at: timestamp, records: count, pages: count, attempts: count, required: z.boolean() });
export const operationPage = paged(operation);
export const evidence = z.object({ id: z.string(), check, operation: z.string(), observed_at: timestamp, expires_at: timestamp, context: resourceContext.nullable(), facts });
export const evidencePage = paged(evidence);
export const runPage = z.object({ items: z.array(z.object({ id: z.uuidv7(), target: z.string(), check, started_at: timestamp, finished_at: timestamp, complete: z.boolean(), observations: count, required_failures: count })), next_cursor: z.string().nullable(), gaps: count });
export type Check = z.infer<typeof check>;
export type CheckView = z.infer<typeof checkView>;
export type Coverage = z.infer<typeof coverage>;
export type ResourceContext = z.infer<typeof resourceContext>;

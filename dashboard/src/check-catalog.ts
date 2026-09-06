import type { Check, CheckView, Coverage } from "./schema";
import { stale } from "./format";

/** Every configured check has an operator-facing name and an explicit assessment scope. */
export const checkCatalog: Record<Check, { title: string; description: string }> = {
  preflight: { title: "Access and prerequisites", description: "Verifies credentials, selected identities, target access, and required collection capabilities." },
  discovery: { title: "Organization discovery", description: "Inventories accessible projects, accounts, and subscriptions. Discovery does not establish resource health." },
  inventory: { title: "Resource inventory", description: "Discovers configured cloud resource families and their metadata, including unsupported and inventory-only resources." },
  kubernetes: { title: "Kubernetes health", description: "Checks node and workload readiness, pod restarts, Job outcomes, CronJob schedules, autoscalers, certificates, and secret synchronization." },
  edge: { title: "DNS, TLS, and endpoints", description: "Checks name resolution, trusted certificates, expiry, HTTP status, and latency. Discovered roots establish reachability only." },
  managed: { title: "Managed dependencies", description: "Checks database and cache service state, replicas, backups, recovery configuration, encryption, and maintenance metadata." },
  queues: { title: "Queues and consumers", description: "Correlates backlog and available age metrics with worker readiness, crash loops, and scaler availability." },
  releases: { title: "Release verification", description: "Correlates desired images and running digests with registry metadata, successful builds, repositories, and source commits. Deployment blockers are distinct from service outages." },
  github: { title: "Repository and workflow checks", description: "Collects repositories, open pull requests, configured workflows, deployments, desired build targets, and source revisions." },
  metrics: { title: "Performance and capacity", description: "Evaluates available provider metrics against configured thresholds, valid capacity denominators, and sustained observation windows." },
  logs: { title: "Runtime diagnostics", description: "Samples bounded error and runtime-failure log windows, grouping redacted signatures by source. Samples cannot establish complete error rates or healthy silence." },
  alerts: { title: "Provider alerts and incidents", description: "Checks active monitoring alerts and available provider health incidents. Missing permissions and entitlements remain explicit gaps." },
  slo: { title: "Service objectives", description: "Evaluates configured service objectives, compliance, error budgets, and available burn-rate telemetry." },
  flows: { title: "Processing progress", description: "Correlates configured incoming demand and stage completion signals to detect stalled processing, with idle input treated as expected inactivity." },
};
export const coverageText: Record<Coverage, { title: string; detail: string }> = {
  complete: { title: "Collected", detail: "The operation returned complete evidence for its configured scope." },
  denied: { title: "Access denied", detail: "The selected identity does not have permission for this operation." },
  unauthenticated: { title: "Authentication unavailable", detail: "Credentials are missing, expired, or could not be refreshed." },
  unavailable: { title: "Service unavailable", detail: "The provider or required service could not supply evidence." },
  unsupported: { title: "Not supported", detail: "This operation is not supported for the selected service or scope." },
  missing: { title: "Required evidence missing", detail: "The expected observations or prerequisite evidence were not available." },
  truncated: { title: "Partial results", detail: "Collection reached a response, record, pagination, or retained-state limit. The collector did not record which cutoff was reached." },
  timeout: { title: "Collection timed out", detail: "The operation did not finish within its configured deadline." },
  cancelled: { title: "Collection interrupted", detail: "The operation was cancelled before it could complete." },
  malformed: { title: "Invalid provider response", detail: "The response could not be interpreted as valid evidence." },
  stale: { title: "Evidence is stale", detail: "The original observation is older than its freshness allowance." },
  inventory_only: { title: "Inventory metadata only", detail: "The resource was observed, but this operation does not verify its health or release provenance." },
};
export function checkState(check: CheckView, now: number): "awaiting" | "stale" | "complete" | "incomplete" {
  if (!check.finished_at) return "awaiting";
  if (stale(check.expires_at, now)) return "stale";
  return check.complete ? "complete" : "incomplete";
}
export const checkStateText = { awaiting: "Awaiting first observation", stale: "Stale evidence", complete: "Complete required evidence", incomplete: "Incomplete evidence" };
export function interval(seconds: number): string {
  if (seconds >= 3600 && seconds % 3600 === 0) return `${seconds / 3600} hour${seconds === 3600 ? "" : "s"}`;
  if (seconds >= 60 && seconds % 60 === 0) return `${seconds / 60} minute${seconds === 60 ? "" : "s"}`;
  return `${seconds} seconds`;
}
export function checkPath(target: string, check: Check): string { return `/checks/${encodeURIComponent(target)}/${check}?target=${encodeURIComponent(target)}`; }
export function targetPath(target: string): string { return `/targets/${encodeURIComponent(target)}?target=${encodeURIComponent(target)}`; }

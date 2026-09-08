import { expect } from "@playwright/test";
import { z } from "zod";
import type { Page } from "@playwright/test";
import type { Check, Overview } from "../../src/schema";
const kinds: Check[] = ["preflight", "inventory", "kubernetes", "edge", "managed", "queues", "releases", "github", "metrics", "logs", "alerts", "slo"];
export async function fixture(page: Page) {
  const diagnostics: string[] = [];
  page.on("console", (message) => { if (/\[(?:STRICT_|PENDING_|REACTIVE_|NO_OWNER_|REACTIVITY_HALTED)/.test(message.text())) diagnostics.push(message.text()); });
  page.on("pageerror", (error) => diagnostics.push(error.message));
  await page.addInitScript(() => {
    class Stream extends EventTarget {
      onopen: ((event: Event) => void) | null = null;
      onerror: ((event: Event) => void) | null = null;
      private timer: number;
      constructor() { super(); this.timer = window.setInterval(() => this.dispatchEvent(new MessageEvent("revision", { data: JSON.stringify({ generation: Math.floor(Date.now() / 5000), epoch: "fixture" }) })), 5000); queueMicrotask(() => this.onopen?.(new Event("open"))); }
      close() { window.clearInterval(this.timer); }
    }
    Object.defineProperty(window, "EventSource", { value: Stream });
  });
  const at = new Date().toISOString();
  const expires = new Date(Date.now() + 3600_000).toISOString();
  const checks = kinds.map((check) => ({ key: `dev/${check}`, target: "dev", check, interval_seconds: 300, started_at: at, finished_at: at, expires_at: expires, complete: check !== "releases", observations: 137, required_failures: check === "releases" ? 1 : 0, optional_gaps: check === "releases" ? 2 : 0, prerequisite: false,
    failures: check === "releases" ? [{ operation: "builds/global", coverage: "truncated" as const, required: true }, { operation: "provenance/external/image-one", coverage: "inventory_only" as const, required: false }, { operation: "provenance/external/image-two", coverage: "inventory_only" as const, required: false }] : [],
  }));
  const overview: Overview = { total_problem_groups: 0, problem_groups: [], generation: 1, configuration_revision: "fixture", captured_at: at, heartbeat_at: at, running: true, persistence_fault: false,
    history: { available: true, last_persisted_at: at, dropped_events: 0, dropped_runs: 0, queued_batches: 0, gaps: 0 },
    totals: { targets: 1, resources: 1, error_findings: 1, warning_findings: 0, incomplete_checks: 1 },
    targets: [{ name: "dev", provider: "gcp", scope: "example", regions: ["us-central1"], health: "unhealthy", complete_checks: 11, total_checks: 12, errors: 1, warnings: 0, resources: 1, latest_observation: at }], checks,
  };
  const context = { provider: "gcp", scope: "example", service: "cloud-run", native_id: "projects/example/locations/us-central1/services/api", region: "us-central1", zone: null, cluster: null, namespace: null, name: "api", uid: null, container: null };
  const links = [{ label: "Open in Google Cloud", url: "https://console.cloud.google.com/run/detail/us-central1/api/metrics?project=example" }];
  const facts = [{ label: "HTTP status", value: "503" }, { label: "Accepted HTTP statuses", value: "200" }];
  const resource = { id: "dev/endpoints/api", target: "dev", check: "edge", checks: ["edge", "inventory"], health: "unhealthy", expected: "active", observed_at: at, expires_at: expires, context, links, facts };
  const finding = { id: "dev/endpoints/api/endpoint-unreachable", target: "dev", resource: resource.id, check: "edge", rule: "endpoint-unreachable", severity: "error", observed_at: at, valid_until: expires, expected: "active", confidence: "direct", stale: false, evidence: ["endpoints"], diagnostic: { first_detected_at: "2026-09-05T00:00:00Z", last_detected_at: at, context, facts, links } };
  const operations = Array.from({ length: 137 }, (_, i) => ({ id: `operation-${String(i).padStart(4, "0")}`, coverage: i === 0 ? "truncated" : "complete", required: true, observed_at: at, records: 10, pages: 1, attempts: 1 }));
  await page.route("**/api/v1/overview**", (route) => route.fulfill({ json: overview }));
  await page.route("**/api/v1/query/costs/**", (route) => route.fulfill({ status: 503, json: { error: "Billing is disabled in this fixture" } }));
  await page.route("**/api/v1/checks?**", (route) => route.fulfill({ json: { generation: 1, items: checks, next_cursor: null, previous_cursor: null, total: 12 } }));
  await page.route("**/api/v1/check?**", (route) => route.fulfill({ json: checks.find((check) => check.check === "releases") }));
  await page.route("**/api/v1/check/operations?**", (route) => {
    const url = new URL(route.request().url());
    const q = url.searchParams.get("q") ?? "";
    const selected = operations.filter((op) => op.id.includes(q));
    const raw = url.searchParams.get("cursor");
    const cursor = raw ? z.object({ resource: z.string() }).parse(JSON.parse(raw) as unknown).resource : undefined;
    const start = cursor ? selected.findIndex((op) => op.id === cursor) + 1 : 0;
    const items = selected.slice(start, start + 50);
    const last = items.at(-1);
    return route.fulfill({ json: { generation: 1, items, total: selected.length, previous_cursor: null, next_cursor: start + items.length < selected.length && last ? JSON.stringify({ priority: 0, resource: last.id, identity: "" }) : null } });
  });
  await page.route("**/api/v1/resources?**", (route) => route.fulfill({ json: { generation: 1, items: [{ ...resource, findings: [finding], finding_count: 1 }], next_cursor: null, previous_cursor: null, total: 1 } }));
  await page.route("**/api/v1/resource?**", (route) => route.fulfill({ json: { generation: 1, resource, findings: [] } }));
  await page.route("**/api/v1/resource/evidence?**", (route) => route.fulfill({ json: { generation: 1, items: [
    { id: "edge/endpoints", check: "edge", operation: "endpoints", observed_at: at, expires_at: expires, context, facts },
    { id: "inventory/service", check: "inventory", operation: "service", observed_at: at, expires_at: expires, context, facts: [{ label: "Family", value: "Cloud Run" }] },
  ], next_cursor: null, previous_cursor: null, total: 2 } }));
  await page.route("**/api/v1/findings?**", (route) => route.fulfill({ json: { generation: 1, items: [finding], total: 1, next_cursor: null, previous_cursor: null } }));
  await page.route("**/api/v1/runs?**", (route) => route.fulfill({ json: { items: [], next_cursor: null, gaps: 0 } }));
  await page.route("**/api/v1/history?**", (route) => route.fulfill({ json: { items: [], next_cursor: null, gaps: 0 } }));
  return { assertReactiveDiagnostics: () => expect(diagnostics).toEqual([]) };
}

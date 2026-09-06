import { useSearchParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query, resourcePath } from "./api";
import { resourcePage } from "./schema";
import { age, rule, stale } from "./format";
import { Empty, Notice, Pagination, Status } from "./components";

export default function ResourcesPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = useQuery(() => query("/api/v1/resources", { target: dashboard.target(), q: value("q"), health: value("health"), cursor: value("cursor") }), resourcePage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><span class="eyebrow">OBSERVED INFRASTRUCTURE</span><h1>Resources</h1><p>Current evidence from the configured monitoring scope.</p></div></div>
    <div class="filters"><label>Search<input type="search" placeholder="Resource name or identifier" value={value("q")} maxlength={128} onInput={(event) => setParams({ q: event.currentTarget.value || undefined, cursor: undefined })} /></label><label>Health<select value={value("health")} onChange={(event) => setParams({ health: event.currentTarget.value || undefined, cursor: undefined })}><option value="">All states</option><option value="unhealthy">Unhealthy</option><option value="degraded">Degraded</option><option value="unknown">Unknown</option><option value="healthy">Healthy</option><option value="expected_inactive">Expected inactive</option></select></label><Show when={result.loading()}><span class="loading-label">Updating…</span></Show></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice></Show>
    <section class="panel" aria-busy={result.loading() ? "true" : "false"}><Show when={result.data()} fallback={<Empty title="Loading resources" />}>{(page) => <>
      <Show when={page().items.length} fallback={<Empty title="No matching resources" detail="Try another target, health state, or search." />}><div class="table-scroll"><table><thead><tr><th>Resource</th><th>Target</th><th>Health</th><th>Findings</th><th>Observation</th><th>Freshness</th></tr></thead><tbody><For each={page().items}>{(resource) => <tr>
        <td><a class="primary-link resource-name" href={resourcePath(resource.id, dashboard.target())}>{resource.id}</a><span class="cell-detail">{rule(resource.check)}</span></td><td>{resource.target}</td>
        <td><Status health={stale(resource.expires_at, dashboard.now()) ? "unknown" : resource.health} /></td>
        <td class="resource-findings"><For each={resource.findings} fallback={<span class="muted">No active findings</span>}>{(finding) => <div class="resource-finding"><a class={`finding-rule ${finding.severity === "error" ? "text-error" : finding.severity === "warning" ? "text-warning" : "muted"}`} href={resourcePath(resource.id, dashboard.target())}>{rule(finding.rule)}</a><span class="cell-detail">{rule(finding.severity)} · {finding.stale || stale(finding.valid_until, dashboard.now()) ? "Stale evidence" : age(finding.observed_at, dashboard.now())}</span></div>}</For><Show when={resource.finding_count > resource.findings.length}><a class="quiet-link" href={resourcePath(resource.id, dashboard.target())}>+{resource.finding_count - resource.findings.length} more</a></Show></td>
        <td><For each={resource.facts.slice(0, resource.finding_count ? 3 : 1)} fallback={<span class="muted">Metadata observed</span>}>{(fact) => <div class="resource-fact"><span>{fact.label}</span><span class="cell-detail">{fact.value}</span></div>}</For></td>
        <td><span class={stale(resource.expires_at, dashboard.now()) ? "text-warning" : ""}>{stale(resource.expires_at, dashboard.now()) ? "Stale" : age(resource.observed_at, dashboard.now())}</span></td>
      </tr>}</For></tbody></table></div></Show>
      <Pagination current={value("cursor")} next={page().next_cursor} total={page().total} onChange={(cursor) => setParams({ cursor: cursor || undefined })} />
    </>}</Show></section>
  </>;
}

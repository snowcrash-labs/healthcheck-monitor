import { useSearchParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query, resourcePath } from "./api";
import { findingPage } from "./schema";
import { age, rule, stale } from "./format";
import { Empty, Notice, Pagination } from "./components";

export default function FindingsPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = useQuery(() => query("/api/v1/findings", { target: dashboard.target(), severity: value("severity"), q: value("q"), cursor: value("cursor") }), findingPage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><span class="eyebrow">HEALTH EVALUATION</span><h1>Findings</h1><p>Active issues remain open until fresh evidence confirms recovery.</p></div></div>
    <div class="filters"><label>Search<input type="search" placeholder="Resource or rule" value={value("q")} maxlength={128} onInput={(event) => setParams({ q: event.currentTarget.value || undefined, cursor: undefined })} /></label><label>Severity<select value={value("severity")} onChange={(event) => setParams({ severity: event.currentTarget.value || undefined, cursor: undefined })}><option value="">All severities</option><option value="error">Error</option><option value="warning">Warning</option><option value="info">Info</option></select></label><Show when={result.loading()}><span class="loading-label">Updating…</span></Show></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice></Show>
    <section class="panel" aria-busy={result.loading() ? "true" : "false"}><Show when={result.data()} fallback={<Empty title="Loading findings" />} >{(page) => <>
      <Show when={page().items.length} fallback={<Empty title="No matching findings" detail="Check coverage and freshness before concluding that the selected scope is healthy." />}><div class="table-scroll"><table><thead><tr><th>Severity</th><th>Finding</th><th>Target</th><th>Evidence</th><th>Last observed</th></tr></thead><tbody><For each={page().items}>{(finding) => <tr><td><span class={`severity severity-${finding.severity}`}>{rule(finding.severity)}</span></td><td><a class="primary-link" href={resourcePath(finding.resource, dashboard.target())}>{rule(finding.rule)}</a><span class="cell-detail resource-name">{finding.resource}</span></td><td>{finding.target}</td><td><span class={finding.stale || stale(finding.valid_until, dashboard.now()) ? "text-warning" : "muted"}>{finding.stale || stale(finding.valid_until, dashboard.now()) ? "Stale" : rule(finding.confidence)}</span><span class="cell-detail">{finding.evidence.length} references</span></td><td>{age(finding.observed_at, dashboard.now())}</td></tr>}</For></tbody></table></div></Show>
      <Pagination current={value("cursor")} next={page().next_cursor} total={page().total} onChange={(cursor) => setParams({ cursor: cursor || undefined })} />
    </>}</Show></section>
  </>;
}

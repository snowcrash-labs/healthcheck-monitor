import { useSearchParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { usePages } from "./use-pages";
import { ScrollBoundary } from "./scroll-boundary";
import { query, resourcePath } from "./api";
import { findingPage } from "./schema";
import { findingTitle } from "./diagnostics";
import { locationText, resourceName } from "./identity";
import { age, rule, stale } from "./format";
import { Empty, Notice } from "./components";

export default function FindingsPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = usePages(() => query("/api/v1/findings", { target: dashboard.target(), severity: value("severity"), check: value("check"), rule: value("rule"), resource: value("resource"), q: value("q") }), findingPage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><h1>Problems</h1><p>{dashboard.target() || "All configured targets"} · Unresolved problems, regardless of when they began</p></div><span class="count-label">{result.data()?.total.toLocaleString() ?? "…"} active</span></div>
    <nav class="tabs" aria-label="Problem views"><a class="tab" aria-current="page" href={query("/problems", { target: dashboard.target() })}>Active</a><a class="tab" href={query("/recent-errors", { target: dashboard.target() })}>Recent errors</a><a class="tab" href={query("/history", { target: dashboard.target() })}>Changes</a></nav>
    <div class="filters"><label>Search problems<input type="search" placeholder="Resource or problem" value={value("q")} maxlength={128} onInput={(event) => setParams({ q: event.currentTarget.value || undefined })} /></label><label>Severity<select value={value("severity")} onChange={(event) => setParams({ severity: event.currentTarget.value || undefined })}><option value="">All severities</option><option value="error">Error</option><option value="warning">Warning</option><option value="info">Info</option></select></label><Show when={value("rule") || value("check") || value("resource") || value("q") || value("severity")}><button class="filter-chip" onClick={() => setParams({ rule: undefined, check: undefined, resource: undefined, q: undefined, severity: undefined })}>{value("rule") ? findingTitle(value("rule")) + " · " : ""}Clear filters ×</button></Show><span class="loading-label" aria-live="polite">{result.loading() ? "Updating…" : ""}</span></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show>
    <section class="panel" aria-busy={result.loading() ? "true" : "false"}><Show when={result.data()} fallback={<Empty title="Loading problems" />}>{(page) => <>
      <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
      <Show when={page().items.length} fallback={<Empty title="No matching active problems" detail="Check collection coverage before concluding this scope is healthy." />}><div class="table-scroll"><table class="problems-table"><thead><tr><th>Problem</th><th>Resource / location</th><th>Severity</th><th>First detected</th><th>Last confirmed</th></tr></thead><tbody><For each={page().items} keyed={(finding) => finding.id}>{(finding) => <tr data-row-key={finding().id}>
        <td><a class="primary-link" href={resourcePath(finding().resource, dashboard.target())}>{findingTitle(finding().rule)}</a><Show when={finding().diagnostic?.facts[0]}>{(fact) => <span class="cell-detail">{fact().label}: {fact().value}</span>}</Show></td>
        <td><a href={resourcePath(finding().resource, dashboard.target())}>{resourceName(finding().resource, finding().diagnostic?.context)}</a><span class="cell-detail">{finding().target} · {locationText(finding().diagnostic?.context)}</span></td>
        <td><span class={`severity severity-${finding().severity}`}>{rule(finding().severity)}</span></td>
        <td><time datetime={finding().diagnostic?.first_detected_at ?? undefined}>{age(finding().diagnostic?.first_detected_at ?? null, dashboard.now())}</time></td>
        <td><time datetime={finding().diagnostic?.last_detected_at ?? finding().observed_at}>{age(finding().diagnostic?.last_detected_at ?? finding().observed_at, dashboard.now())}</time><Show when={finding().stale || stale(finding().valid_until, dashboard.now())}><span class="cell-detail text-warning">Stale evidence</span></Show></td>
      </tr>}</For></tbody></table></div></Show>
      <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
    </>}</Show></section>
  </>;
}

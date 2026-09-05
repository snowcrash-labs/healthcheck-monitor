import { useSearchParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query, resourcePath } from "./api";
import { historyPage } from "./schema";
import { rule, utc } from "./format";
import { Empty, Notice, Pagination } from "./components";

export default function HistoryPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = useQuery(() => query("/api/v1/history", { target: dashboard.target(), resource: value("resource"), kind: value("kind"), before: value("cursor") }), historyPage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><span class="eyebrow">PERSISTED OBSERVATIONS</span><h1>Finding history</h1><p>New issues, recoveries, stale evidence, and confirmed removals.</p></div></div>
    <div class="filters"><label>Transition<select value={value("kind")} onChange={(event) => setParams({ kind: event.currentTarget.value || undefined, cursor: undefined })}><option value="">All transitions</option><option value="new">New</option><option value="worsened">Worsened</option><option value="recovered">Recovered</option><option value="stale">Stale</option><option value="removed">Removed</option><option value="reappeared">Reappeared</option></select></label><Show when={value("resource")}><button class="filter-chip" onClick={() => setParams({ resource: undefined, cursor: undefined })}>Resource filter ×</button></Show><Show when={result.loading()}><span class="loading-label">Updating…</span></Show></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice></Show>
    <Show when={(result.data()?.gaps ?? 0) > 0}><Notice>History contains recorded delivery gaps. Some transitions could not be retained.</Notice></Show>
    <section class="panel" aria-busy={result.loading() ? "true" : "false"}><Show when={result.data()} fallback={<Empty title={result.error() ? "History unavailable" : "Loading history"} detail="Live monitoring continues independently of historical storage." />}>{(page) => <>
      <Show when={page().items.length} fallback={<Empty title="No matching transitions" detail="New findings and confirmed changes will appear here as checks run." />}><div class="timeline"><For each={page().items}>{(event) => <article class="timeline-event"><div class={`timeline-marker transition-${event.kind}`} /><div class="timeline-main"><div class="timeline-title"><span class={`transition transition-${event.kind}`}>{rule(event.kind)}</span><a class="primary-link" href={resourcePath(event.resource, dashboard.target())}>{rule(event.rule)}</a><span class={`severity severity-${event.severity}`}>{rule(event.severity)}</span></div><p class="resource-path">{event.resource}</p><small>{event.target} · {rule(event.confidence)} evidence</small></div><time datetime={event.at}>{utc(event.at)}</time></article>}</For></div></Show>
      <Pagination current={value("cursor")} next={page().next_cursor} onChange={(cursor) => setParams({ cursor: cursor || undefined })} />
    </>}</Show></section>
  </>;
}

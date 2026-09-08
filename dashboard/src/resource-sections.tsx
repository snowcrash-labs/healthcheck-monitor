import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { query } from "./api";
import { evidencePage, findingPage, historyPage } from "./schema";
import { usePages } from "./use-pages";
import { useQuery } from "./use-query";
import { useSearchParams } from "@solidjs/router";
import { Empty, Notice, Pagination } from "./components";
import { Facts, FindingDetail, Timestamp } from "./diagnostics";
import { checkCatalog, checkPath } from "./check-catalog";
import { ScrollBoundary } from "./scroll-boundary";
import { rule, stale } from "./format";

export function ResourceFindings(props: { id: string; target: string }) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const result = usePages(() => query("/api/v1/findings", { target: props.target, resource: props.id }), findingPage, dashboard.refreshId);
  return <section class="panel"><div class="panel-heading"><h2>Current problems</h2><span>{result.data()?.total ?? "…"} active</span></div><Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show><Show when={result.data()}>{(page) => <>
    <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
    <For each={page().items} keyed={(row) => row.id} fallback={<Empty title="No active findings for this resource" />}>{(finding) => <div data-row-key={finding().id}><FindingDetail finding={finding()} now={dashboard.now()} /></div>}</For>
    <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
  </>}</Show></section>;
}
export function ResourceEvidence(props: { id: string; target: string; check?: "logs" }) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [search, setSearch] = useSearchParams();
  const q = () => typeof search.evidence === "string" ? search.evidence : "";
  const result = usePages(() => query("/api/v1/resource/evidence", { id: props.id, q: q(), check: props.check }), evidencePage, dashboard.refreshId);
  return <section class="panel"><div class="panel-heading"><div><h2>Observed facts</h2><p>Grouped by source check and component; each observation retains its own age.</p></div><label>Search evidence<input type="search" value={q()} onInput={(event) => setSearch({ evidence: event.currentTarget.value || undefined })} /></label></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show>
    <Show when={result.data()}>{(page) => <><Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
      <For each={page().items} keyed={(row) => row.id} fallback={<Empty title="No matching evidence" />}>{(evidence) => <article class="operation-detail" data-row-key={evidence().id}><h3><a href={checkPath(props.target, evidence().check)}>{checkCatalog[evidence().check].title}</a></h3><p>{evidence().operation}</p><Timestamp at={evidence().observed_at} now={dashboard.now()} /><Show when={stale(evidence().expires_at, dashboard.now())}><p class="text-warning">Stale evidence</p></Show><Facts values={evidence().facts} /></article>}</For>
      <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
    </>}</Show>
  </section>;
}
export function ResourceHistory(props: { id: string; target: string }) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const before = () => typeof params.history === "string" ? params.history : "";
  const result = useQuery(() => query("/api/v1/history", { target: props.target, resource: props.id, before: before() }), historyPage, dashboard.refreshId);
  return <section class="panel"><div class="panel-heading"><h2>Finding timeline</h2></div><Show when={result.error()}><Notice error>{result.error()}</Notice></Show><Show when={result.data()}>{(page) => <>
    <Show when={page().gaps > 0}><Notice>History contains recorded delivery gaps.</Notice></Show><For each={page().items} keyed={(row) => row.id} fallback={<Empty title="No retained transitions" />}>{(event) => <article class="operation-detail"><strong>{rule(event().kind)} · {rule(event().rule)}</strong><Timestamp at={event().at} now={dashboard.now()} /></article>}</For><Pagination current={before()} next={page().next_cursor} onChange={(cursor) => setParams({ history: cursor || undefined })} />
  </>}</Show></section>;
}

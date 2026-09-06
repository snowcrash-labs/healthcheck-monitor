import { For, Show } from "solid-js";
import { useParams, useSearchParams } from "@solidjs/router";
import { useDashboard } from "./context";
import { query } from "./api";
import { check as checkSchema, checkView, operationPage, runPage } from "./schema";
import { usePages } from "./use-pages";
import { useQuery } from "./use-query";
import { Empty, Notice, Pagination } from "./components";
import { Timestamp } from "./diagnostics";
import { checkCatalog, checkState, checkStateText, coverageText, interval, targetPath } from "./check-catalog";
import { ScrollBoundary } from "./scroll-boundary";
import { rule } from "./format";
export default function CheckDetailPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const params = useParams();
  const [search, setSearch] = useSearchParams();
  const selected = () => checkSchema.safeParse(params.check);
  const kind = () => { const parsed = selected(); return parsed.success ? parsed.data : undefined; };
  const target = () => params.target ?? "";
  const q = () => typeof search.q === "string" ? search.q : "";
  const before = () => typeof search.before === "string" ? search.before : "";
  const check = useQuery(() => query("/api/v1/check", { target: target(), check: kind() }), checkView, dashboard.refreshId);
  const operations = usePages(() => query("/api/v1/check/operations", { target: target(), check: kind(), q: q() }), operationPage, dashboard.refreshId);
  const runs = useQuery(() => query("/api/v1/runs", { target: target(), check: kind(), before: before() }), runPage, dashboard.refreshId);
  return <><a class="back-link" href={query("/checks", { target: target() })}>← Checks</a>
    <Show when={check.error()}><Notice error>{check.error()}</Notice></Show>
    <Show when={check.data()} fallback={<Empty title={check.error() ? "Check unavailable" : "Loading check"} />}>{(row) => <>
      <div class="page-heading"><div><span class="eyebrow"><a href={targetPath(target())}>{target()}</a> · Every {interval(row().interval_seconds)}</span><h1>{checkCatalog[row().check].title}</h1><p>{checkCatalog[row().check].description}</p></div><span class={checkState(row(), dashboard.now()) === "complete" ? "text-healthy" : "text-warning"}>{checkStateText[checkState(row(), dashboard.now())]}</span></div>
      <div class="detail-meta"><span>Started <Timestamp at={row().started_at} now={dashboard.now()} /></span><span>Finished <Timestamp at={row().finished_at} now={dashboard.now()} /></span><span>{row().observations.toLocaleString()} observations</span><Show when={row().started_at && row().finished_at}><span>Run duration {Math.max(0, (Date.parse(row().finished_at ?? "") - Date.parse(row().started_at ?? "")) / 1000).toFixed(1)} seconds</span></Show></div>
      <p>{row().required_failures} required operations lack complete evidence; {row().optional_gaps} optional coverage gaps.<Show when={row().prerequisite}> Included as a collection prerequisite.</Show></p>
      <nav class="detail-nav"><a class="button" href={query("/resources", { target: target(), check: kind() })}>Observed resources</a><a class="button" href={query("/findings", { target: target(), check: kind() })}>Related findings</a><a class="button" href={query("/checks", { target: target() })}>Other checks and prerequisites</a></nav>
      <section class="panel"><div class="panel-heading"><h2>Collection operations</h2><label>Search operations<input type="search" value={q()} onInput={(e) => setSearch({ q: e.currentTarget.value || undefined })} /></label></div>
        <Show when={operations.error()}><Notice error>{operations.error()}</Notice><button class="button" onClick={operations.retry}>Retry</button></Show>
        <Show when={operations.data()}>{(page) => <><Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={operations.loading()} load={operations.previous} /></Show>
          <For each={page().items} fallback={<Empty title="No operations observed" />} >{(op) => <article class="operation-detail" data-row-key={op.id}><div><h3>{rule(op.id)}</h3><span class={op.coverage === "complete" ? "text-healthy" : "text-warning"}>{coverageText[op.coverage].title}</span></div><p>{coverageText[op.coverage].detail}</p><p>{op.required ? "Required for this check’s coverage" : "Optional; does not prevent complete required coverage"}</p><div class="detail-meta"><span>{op.records.toLocaleString()} records</span><span>{op.pages} pages</span><span>{op.attempts} attempts</span><Timestamp at={op.observed_at} now={dashboard.now()} /></div></article>}</For>
          <ScrollBoundary enabled={!!page().next_cursor} loading={operations.loading()} total={page().total} load={operations.next} />
        </>}</Show>
      </section>
      <section class="panel"><div class="panel-heading"><h2>Recent runs</h2></div><Show when={runs.error()}><Notice error>{runs.error()}</Notice></Show><Show when={runs.data()}>{(page) => <><Show when={page().gaps > 0}><Notice>Recorded history contains delivery gaps.</Notice></Show><For each={page().items} fallback={<Empty title="No retained runs" />} >{(run) => <article class="operation-detail"><strong>{run.complete ? "Required collection completed" : "Collection incomplete"}</strong><p>{run.observations.toLocaleString()} observations · {run.required_failures} required gaps</p><Timestamp at={run.finished_at} now={dashboard.now()} /></article>}</For><Pagination current={before()} next={page().next_cursor} onChange={(cursor) => setSearch({ before: cursor || undefined })} /></>}</Show></section>
    </>}</Show></>;
}

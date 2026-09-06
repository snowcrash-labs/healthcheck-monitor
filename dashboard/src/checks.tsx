import { For, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { useDashboard } from "./context";
import { query } from "./api";
import { checkPage } from "./schema";
import { usePages } from "./use-pages";
import { CheckCard } from "./check-card";
import { Empty, Notice } from "./components";
import { ScrollBoundary } from "./scroll-boundary";
export default function ChecksPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = usePages(() => query("/api/v1/checks", { target: dashboard.target(), q: value("q"), status: value("status") }), checkPage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><span class="eyebrow">COLLECTION AND EVALUATION</span><h1>Checks</h1><p>Every configured check, its purpose, and the evidence collected.</p></div></div>
    <div class="filters"><label>Search checks<input type="search" value={value("q")} onInput={(e) => setParams({ q: e.currentTarget.value || undefined })} /></label><label>Coverage<select value={value("status")} onChange={(e) => setParams({ status: e.currentTarget.value || undefined })}><option value="">All checks</option><option value="complete">Complete required evidence</option><option value="needs_attention">Missing, incomplete, or stale</option><option value="incomplete">Incomplete evidence</option><option value="stale">Stale evidence</option><option value="awaiting">Awaiting first observation</option></select></label></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show>
    <Show when={result.data()} fallback={<Empty title="Loading checks" />}>{(page) => <>
      <p>{page().total} configured checks in this selection. Collection completeness does not establish resource health.</p>
      <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
      <div class="check-grid"><For each={page().items} fallback={<Empty title="No matching checks" />}>{(check) => <div data-row-key={check.id}><CheckCard check={check} now={dashboard.now()} /></div>}</For></div>
      <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
    </>}</Show>
  </>;
}

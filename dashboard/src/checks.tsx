import { Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { useDashboard } from "./context";
import { query } from "./api";
import { checkPage } from "./schema";
import { usePages } from "./use-pages";
import { CheckTable } from "./check-table";
import { Empty, Notice } from "./components";
import { ScrollBoundary } from "./scroll-boundary";

export default function ChecksPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const result = usePages(() => query("/api/v1/checks", { target: dashboard.target(), q: value("q"), status: value("status") }), checkPage, dashboard.refreshId);
  return <>
    <div class="page-heading"><div><h1>Checks</h1><p>Collection coverage for {dashboard.target() || "all configured targets"}.</p></div><span class="scope-tag">Current state</span></div>
    <div class="filters"><label>Search checks<input type="search" placeholder="Name or purpose" maxlength={128} value={value("q")} onInput={(e) => setParams({ q: e.currentTarget.value || undefined })} /></label><label>Collection<select value={value("status")} onChange={(e) => setParams({ status: e.currentTarget.value || undefined })}><option value="">All checks</option><option value="needs_attention">Needs attention</option><option value="complete">Collected</option><option value="awaiting">Waiting</option><option value="stale">Stale</option></select></label></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show>
    <section class="panel"><div class="panel-heading"><h2>{result.data()?.total ?? "…"} checks</h2><span class="muted">Attention first · Collection is separate from health</span></div>
      <Show when={result.data()} fallback={<Empty title="Loading checks" />}>{(page) => <>
        <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
        <Show when={page().items.length} fallback={<Empty title="No matching checks" />}><CheckTable checks={page().items} now={dashboard.now()} /></Show>
        <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
      </>}</Show>
    </section>
  </>;
}

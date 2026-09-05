import { Errored, For, Loading, Show } from "solid-js";
import type { ParentProps } from "solid-js";
import { DashboardProvider, useDashboard } from "./context";
import { age } from "./format";
import { Empty, Notice } from "./components";
import { ThemePicker } from "./theme";
import { query } from "./api";

function Layout(props: ParentProps) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const path = (route: string) => query(route, { target: dashboard.target() });
  return <div class="app-shell">
    <a class="skip-link" href="#main-content">Skip to content</a>
    <aside class="sidebar">
      <a href={path("/")} class="brand" aria-label="Soundpatrol health overview"><svg viewBox="0 0 28 28" aria-hidden="true"><path d="M3 11v6m5-11v16m6-20v24m6-19v14m5-10v6" /></svg><span>Soundpatrol<small>System health</small></span></a>
      <span class="nav-label">MONITORING</span>
      <nav aria-label="Main navigation"><a href={path("/")}><span aria-hidden="true">◫</span>Overview</a><a href={path("/findings")}><span aria-hidden="true">◉</span>Findings</a><a href={path("/resources")}><span aria-hidden="true">▦</span>Resources</a><a href={path("/history")}><span aria-hidden="true">◷</span>History</a></nav>
      <div class="sidebar-foot"><span class={`connection-dot ${dashboard.connected() ? "online" : "offline"}`} /><span>{dashboard.connected() ? "Live connection" : "Reconnecting"}</span><small>Read-only monitoring</small></div>
    </aside>
    <div class="workspace">
      <header class="topbar"><div class="breadcrumb">Operations <span>/</span> Infrastructure</div><div class="topbar-actions"><span class="updated">{age(dashboard.overview()?.captured_at ?? null, dashboard.now())}</span><label class="target-picker">Target<select aria-label="Monitoring target" value={dashboard.target()} onChange={(event) => dashboard.setTarget(event.currentTarget.value)}><option value="">All targets</option><For each={dashboard.overview()?.targets ?? []}>{(target) => <option value={target.name}>{target.name}</option>}</For></select></label><ThemePicker /><button class="button refresh-button" onClick={dashboard.refresh}>↻ <span>Refresh view</span></button></div></header>
      <main id="main-content" tabindex={-1}>
        <Show when={!dashboard.connected() && dashboard.overview()}><Notice>Connection interrupted. Displayed observations retain their original timestamps.</Notice></Show>
        <Show when={dashboard.error()}><Notice error>{dashboard.error()}</Notice></Show>
        <Show when={dashboard.overview() && (!dashboard.overview()?.running || dashboard.now() - Date.parse(dashboard.overview()?.heartbeat_at ?? "1970-01-01T00:00:00Z") > 15_000)}><Notice>The monitor heartbeat is stale or stopped. Displayed health requires fresh collection.</Notice></Show>
        <Show when={dashboard.overview()?.view_truncated}><Notice>The dashboard reached its view limit. Coverage is incomplete; narrow the configured scope or increase the view budget.</Notice></Show>
        <Show when={dashboard.overview()?.persistence_fault}><Notice error>Evidence publication is failing. Live checks continue while storage retries.</Notice></Show>
        <Errored fallback={(_, reset) => <div class="empty"><h2>This view could not be displayed.</h2><button class="button" onClick={reset}>Try again</button></div>}><Loading fallback={<Empty title="Loading view" />}>{props.children}</Loading></Errored>
      </main>
      <footer class="page-footer"><span>Health, coverage, and freshness are evaluated separately.</span><span>Times in UTC</span></footer>
    </div>
  </div>;
}
export function Root(props: ParentProps) { return <DashboardProvider><Layout>{props.children}</Layout></DashboardProvider>; }

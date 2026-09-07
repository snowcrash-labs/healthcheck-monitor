import { Errored, For, Loading, Show } from "solid-js";
import type { ParentProps } from "solid-js";
import { useLocation } from "@solidjs/router";
import { DashboardProvider, useDashboard } from "./context";
import { age, utc } from "./format";
import { Empty, Notice } from "./components";
import { ThemePicker } from "./theme";
import { query } from "./api";

const primary = [{ path: "/", label: "Overview" }, { path: "/problems", label: "Problems" }, { path: "/resources", label: "Resources" }, { path: "/costs", label: "Costs" }];
const secondary = [{ path: "/checks", label: "Checks" }, { path: "/monitor", label: "Monitor status" }];
function Layout(props: ParentProps) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const location = useLocation();
  const path = (route: string) => query(route, { target: dashboard.target() });
  const active = (route: string) => route === "/" ? location.pathname === "/" || location.pathname.startsWith("/targets/") : location.pathname.startsWith(route) || route === "/problems" && ["/findings", "/history"].includes(location.pathname);
  const page = () => [...primary, ...secondary].find((route) => active(route.path))?.label ?? "Resource";
  return <div class="app-shell">
    <a class="skip-link" href="#main-content">Skip to content</a>
    <aside class="sidebar">
      <a href={path("/")} class="brand" aria-label="Soundpatrol health overview"><svg viewBox="0 0 28 28" aria-hidden="true"><path d="M3 11v6m5-11v16m6-20v24m6-19v14m5-10v6" /></svg><span>Soundpatrol<small>Health monitor</small></span></a>
      <nav aria-label="Main navigation"><For each={primary}>{(route) => <a href={path(route.path)} aria-current={active(route.path) ? "page" : undefined}>{route.label}</a>}</For></nav>
      <nav class="secondary-nav" aria-label="Monitoring tools"><For each={secondary}>{(route) => <a href={path(route.path)} aria-current={active(route.path) ? "page" : undefined}>{route.label}</a>}</For></nav>
      <div class="sidebar-foot"><span class={`connection-dot ${dashboard.connected() ? "online" : "offline"}`} /><span>{dashboard.connected() ? "Connected" : "Reconnecting"}</span><small>Read-only monitoring</small></div>
    </aside>
    <div class="workspace">
      <header class="topbar"><div class="breadcrumb">Health <span>/</span> {page()}</div><div class="topbar-actions"><span class="updated">{age(dashboard.overview()?.captured_at ?? null, dashboard.now())}</span><label class="target-picker">Scope<select aria-label="Monitoring target" value={dashboard.target()} onChange={(event) => dashboard.setTarget(event.currentTarget.value)}><option value="" selected={!dashboard.target()}>All targets</option><For each={dashboard.targets()} keyed={(target) => target.name}>{(target) => <option value={target().name} selected={target().name === dashboard.target()}>{target().name}</option>}</For></select></label><ThemePicker /><button class="button" aria-pressed={dashboard.paused() ? "true" : "false"} onClick={dashboard.togglePause}>{dashboard.paused() ? "Resume live" : "Pause"}</button><button class="button refresh-button" disabled={dashboard.paused()} onClick={dashboard.refresh} aria-label="Refresh view">↻</button></div></header>
      <main id="main-content" tabindex={-1}>
        <Show when={dashboard.authorized()} fallback={<Notice error>Your sign-in expired or access was removed. <a href={location.pathname + location.search}>Sign in again</a></Notice>}>
          <Show when={dashboard.paused()}><Notice>Loaded view paused at {utc(new Date(dashboard.now()).toISOString())}. Collection continues.</Notice></Show>
          <Show when={!dashboard.connected() && dashboard.overview()}><Notice>Reconnecting. Showing the last received observations.</Notice></Show>
          <Show when={dashboard.error()}><Notice error>{dashboard.error()}</Notice></Show>
          <Show when={dashboard.overview() && !dashboard.overview()?.running}><Notice>Collection stopped. <a href="/monitor">Monitor status</a></Notice></Show>
          <Show when={dashboard.overview()?.persistence_fault}><Notice error>Evidence storage is failing. <a href="/monitor">Monitor status</a></Notice></Show>
          <Errored fallback={(_, reset) => <div class="empty"><h2>This view could not be displayed.</h2><button class="button" onClick={reset}>Try again</button></div>}><Loading fallback={<Empty title="Loading view" />}>{props.children}</Loading></Errored>
        </Show>
      </main>
    </div>
  </div>;
}
export function Root(props: ParentProps) { return <DashboardProvider><Layout>{props.children}</Layout></DashboardProvider>; }

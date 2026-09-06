import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { CheckCard } from "./check-card";
import { targetPath } from "./check-catalog";
import { age } from "./format";
import { Empty, Notice, Status } from "./components";

export default function OverviewPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const targets = () => dashboard.overview()?.targets.filter((target) => !dashboard.target() || target.name === dashboard.target()) ?? [];
  return <>
    <div class="page-heading"><div><span class="eyebrow">CONFIGURED INFRASTRUCTURE</span><h1>System health</h1><p>Current findings and the evidence behind them.</p></div><span class="scope-tag">{dashboard.target() || "All configured targets"}</span></div>
    <Show when={dashboard.overview()} fallback={<Empty title="Waiting for monitoring" detail="Checks run autonomously. The first observations will appear here." />}>{(overview) => <>
      <div class="stats-grid">
        <a href={`/findings?severity=error${dashboard.target() ? `&target=${encodeURIComponent(dashboard.target())}` : ""}`} class="stat-card"><span>Error findings</span><strong class={overview().totals.error_findings ? "text-error" : ""}>{overview().totals.error_findings}</strong><small>{overview().totals.warning_findings} warnings also active</small></a>
        <a class="stat-card" href={`/checks?target=${encodeURIComponent(dashboard.target())}&status=needs_attention`}><span>Incomplete checks</span><strong class={overview().totals.incomplete_checks ? "text-warning" : ""}>{overview().totals.incomplete_checks}</strong><small>Missing, failed, or stale evidence</small></a>
        <a href={`/resources${dashboard.target() ? `?target=${encodeURIComponent(dashboard.target())}` : ""}`} class="stat-card"><span>Observed resources</span><strong>{overview().totals.resources.toLocaleString()}</strong><small>Across {overview().totals.targets} selected targets</small></a>
        <div class="stat-card"><span>History storage</span><strong class={`stat-word ${overview().history.available ? "text-healthy" : "text-warning"}`}>{overview().history.available ? "Available" : "Unavailable"}</strong><small>{age(overview().history.last_persisted_at, dashboard.now())}</small></div>
      </div>
      <Show when={!overview().history.available || overview().history.dropped_events > 0 || overview().history.dropped_runs > 0}><Notice>Historical coverage is incomplete. Current collection continues independently of the history database.</Notice></Show>
      <section class="panel"><div class="panel-heading"><div><h2>Targets</h2><p>Health does not imply complete telemetry.</p></div><span class="count-label">{targets().length} targets</span></div><div class="target-grid"><For each={targets()}>{(target) => <article class="target-card"><a class="target-card-main" href={targetPath(target.name)}><h3>{target.name}</h3><p>{target.provider.toUpperCase()} · {target.scope}</p><Status health={target.health} /><p>{target.errors} errors · {target.warnings} warnings · {target.resources.toLocaleString()} resources</p><small>{age(target.latest_observation, dashboard.now())}</small><span class="quiet-link">Open target →</span></a><a class="target-coverage" href={`/checks?target=${encodeURIComponent(target.name)}`}>{target.complete_checks} of {target.total_checks} checks have complete, fresh required evidence →</a></article>}</For></div></section>
      <section class="panel"><div class="panel-heading"><div><h2>Check coverage</h2><p>Collection failures remain visible alongside health findings.</p></div></div><div class="check-grid"><For each={overview().checks}>{(check) => <CheckCard check={check} now={dashboard.now()} />}</For></div></section>
    </>}</Show>
  </>;
}

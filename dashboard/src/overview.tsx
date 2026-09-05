import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { age, rule, stale } from "./format";
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
        <div class="stat-card"><span>Incomplete checks</span><strong class={overview().totals.incomplete_checks ? "text-warning" : ""}>{overview().totals.incomplete_checks}</strong><small>Missing, failed, or stale evidence</small></div>
        <a href={`/resources${dashboard.target() ? `?target=${encodeURIComponent(dashboard.target())}` : ""}`} class="stat-card"><span>Observed resources</span><strong>{overview().totals.resources.toLocaleString()}</strong><small>Across {overview().totals.targets} selected targets</small></a>
        <div class="stat-card"><span>History storage</span><strong class={`stat-word ${overview().history.available ? "text-healthy" : "text-warning"}`}>{overview().history.available ? "Available" : "Unavailable"}</strong><small>{age(overview().history.last_persisted_at, dashboard.now())}</small></div>
      </div>
      <Show when={!overview().history.available || overview().history.dropped_events > 0 || overview().history.dropped_runs > 0}><Notice>Historical coverage is incomplete. Current collection continues independently of the history database.</Notice></Show>
      <section class="panel"><div class="panel-heading"><div><h2>Targets</h2><p>Health does not imply complete telemetry.</p></div><span class="count-label">{targets().length} targets</span></div><div class="table-scroll"><table><thead><tr><th>Target</th><th>Health</th><th>Coverage</th><th>Last observation</th><th><span class="sr-only">Details</span></th></tr></thead><tbody><For each={targets()}>{(target) => <tr><td><a class="primary-link" href={`/resources?target=${encodeURIComponent(target.name)}`}>{target.name}</a><span class="cell-detail">{target.provider.toUpperCase()} · {target.scope}</span></td><td><Status health={target.health} /></td><td><strong class="coverage-number">{target.complete_checks}<span> / {target.total_checks}</span></strong><span class="cell-detail">checks complete</span></td><td>{age(target.latest_observation, dashboard.now())}</td><td><a class="quiet-link" href={`/findings?target=${encodeURIComponent(target.name)}`}>Findings →</a></td></tr>}</For></tbody></table></div></section>
      <section class="panel"><div class="panel-heading"><div><h2>Check coverage</h2><p>Collection failures remain visible alongside health findings.</p></div></div><div class="check-grid"><For each={overview().checks}>{(check) => <div class="check-card"><div><strong>{rule(check.check)}</strong><span class={`check-result ${check.complete && !stale(check.expires_at, dashboard.now()) ? "text-healthy" : "text-warning"}`}>{check.complete && !stale(check.expires_at, dashboard.now()) ? "Complete" : "Incomplete"}</span></div><p>{check.target} · every {check.interval_seconds}s</p><Show when={check.failures.length > 0}><ul><For each={check.failures.slice(0, 3)}>{(failure) => <li>{rule(failure.coverage)}<span>{failure.required ? "Required" : "Optional"}</span></li>}</For></ul></Show><small>{age(check.finished_at, dashboard.now())}</small></div>}</For></div></section>
    </>}</Show>
  </>;
}

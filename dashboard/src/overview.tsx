import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { targetPath } from "./check-catalog";
import { query } from "./api";
import { age, rule } from "./format";
import { findingTitle } from "./diagnostics";
import { Empty, Notice, Status } from "./components";
import { CostOverview } from "./cost-overview";

export default function OverviewPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const targets = () => dashboard.overview()?.targets.filter((target) => !dashboard.target() || target.name === dashboard.target()) ?? [];
  const path = (route: string, values: Record<string, string> = {}) => query(route, { target: dashboard.target(), ...values });
  return <>
    <div class="page-heading"><div><h1>{dashboard.target() || "Overview"}</h1><p>{dashboard.target() ? targets()[0]?.scope : "All configured targets"} · Current health and recent spending</p></div><span class="scope-tag">Current state</span></div>
    <Show when={dashboard.overview()} fallback={<Empty title="Waiting for monitoring" detail="Observations appear as configured checks finish." />}>{(overview) => <>
      <div class="health-summary"><a href={path("/problems")}><strong class={overview().totals.error_findings ? "text-error" : ""}>{overview().totals.error_findings} errors</strong><span>{overview().totals.warning_findings} warnings</span></a><a href={path("/checks", { status: "needs_attention" })}><strong>{overview().totals.incomplete_checks} checks need attention</strong></a><a href={path("/resources")}>{overview().totals.resources.toLocaleString()} resources</a></div>
      <div class="overview-grid">
        <section class="panel"><div class="panel-heading"><h2>Problems</h2><a href={path("/problems")}>View all →</a></div>
          <For each={overview().problem_groups} keyed={(group) => group.target + "/" + group.rule} fallback={<Empty title={overview().totals.error_findings + overview().totals.warning_findings ? "Problems require investigation" : "No active problems"} detail="Collection coverage is listed separately under Checks." />}>{(group) => <a class="problem-preview" href={query("/problems", { target: group().target, rule: group().rule })}><span class={`severity severity-${group().severity}`}>{rule(group().severity)}</span><div><strong>{findingTitle(group().rule)}</strong><span>{group().target} · {group().resources} {group().resources === 1 ? "resource" : "resources"} · {age(group().last_detected_at, dashboard.now())}{group().stale ? " · Stale evidence" : ""}</span></div></a>}</For>
          <Show when={overview().total_problem_groups > 5}><p class="panel-note">{overview().total_problem_groups - 5} more problem groups</p></Show>
        </section>
        <CostOverview />
      </div>
      <section class="panel"><div class="panel-heading"><h2>Targets</h2><span class="muted">{targets().length} configured scopes</span></div><div class="table-scroll"><table class="targets-table"><thead><tr><th>Target / provider scope</th><th>Health</th><th>Problems</th><th>Checks collected</th><th>Last observation</th></tr></thead><tbody><For each={targets()} keyed={(target) => target.name}>{(target) => <tr><td><a class="primary-link" href={targetPath(target().name)}>{target().name}</a><span class="cell-detail">{target().provider.toUpperCase()} · {target().scope}</span></td><td><Status health={target().health} /></td><td><a href={query("/problems", { target: target().name })}>{target().errors} errors · {target().warnings} warnings</a></td><td><a href={query("/checks", { target: target().name })}>{target().complete_checks} of {target().total_checks} checks</a></td><td><time datetime={target().latest_observation ?? undefined}>{age(target().latest_observation, dashboard.now())}</time></td></tr>}</For></tbody></table></div></section>
    </>}</Show>
  </>;
}

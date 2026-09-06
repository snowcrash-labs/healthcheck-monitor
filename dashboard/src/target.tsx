import { For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { useDashboard } from "./context";
import { query, resourcePath } from "./api";
import { usePages } from "./use-pages";
import { findingPage } from "./schema";
import { Empty, Notice, Status } from "./components";
import { CheckCard } from "./check-card";
import { Timestamp, findingTitle } from "./diagnostics";
import { ScrollBoundary } from "./scroll-boundary";
export default function TargetPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const params = useParams();
  const name = () => params.target ?? "";
  const target = () => dashboard.overview()?.targets.find((target) => target.name === name());
  const problems = usePages(() => query("/api/v1/findings", { target: name() }), findingPage, dashboard.refreshId);
  return <><a class="back-link" href="/">← All targets</a><Show when={target()} fallback={<Empty title={dashboard.overview() ? "Target unavailable" : "Loading target"} />}>{(target) => <>
    <div class="page-heading"><div><span class="eyebrow">{target().provider.toUpperCase()} · {target().scope}</span><h1>{target().name}</h1><p>{target().regions.join(", ") || "Global or source-defined locations"}</p></div><Status health={target().health} /></div>
    <div class="detail-meta"><span>Latest observation <Timestamp at={target().latest_observation} now={dashboard.now()} /></span><a href={query("/checks", { target: name() })}>{target().complete_checks} of {target().total_checks} checks have complete, fresh required evidence →</a></div>
    <nav class="detail-nav" aria-label="Target views"><a class="button" href={query("/checks", { target: name() })}>All checks</a><a class="button" href={query("/resources", { target: name() })}>{target().resources.toLocaleString()} resources</a><a class="button" href={query("/findings", { target: name() })}>{target().errors} errors · {target().warnings} warnings</a><a class="button" href={query("/history", { target: name() })}>History</a></nav>
    <section class="panel"><div class="panel-heading"><h2>Current problems</h2></div><Show when={problems.error()}><Notice error>{problems.error()}</Notice><button class="button" onClick={problems.retry}>Retry</button></Show><Show when={problems.data()}>{(page) => <>
      <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={problems.loading()} load={problems.previous} /></Show>
      <For each={page().items} fallback={<Empty title="No active findings" detail="Check coverage separately for missing or stale evidence." />}>{(finding) => <article class="finding-detail" data-row-key={finding.id}><span class={`severity severity-${finding.severity}`}>{finding.severity}</span><a class="primary-link" href={resourcePath(finding.resource, name())}>{findingTitle(finding.rule)}</a><p class="resource-path">{finding.resource}</p><Timestamp at={finding.diagnostic?.last_detected_at ?? finding.observed_at} now={dashboard.now()} /></article>}</For>
      <ScrollBoundary enabled={!!page().next_cursor} loading={problems.loading()} total={page().total} load={problems.next} />
    </>}</Show></section>
    <section class="panel"><div class="panel-heading"><h2>Configured checks</h2><a href={query("/checks", { target: name() })}>Browse all checks →</a></div><div class="check-grid"><For each={dashboard.overview()?.checks.filter((check) => check.target === name()) ?? []}>{(check) => <CheckCard check={check} now={dashboard.now()} />}</For></div></section>
  </>}</Show></>;
}

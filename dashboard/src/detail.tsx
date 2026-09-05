import { useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query, resourceId } from "./api";
import { resourceDetail } from "./schema";
import { age, rule, stale, utc } from "./format";
import { Empty, Notice, Status } from "./components";

export default function DetailPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const params = useParams();
  const id = () => resourceId(params.id);
  const result = useQuery(() => query("/api/v1/resource", { id: id() }), resourceDetail, dashboard.refreshId);
  return <>
    <a class="back-link" href={query("/resources", { target: dashboard.target() })}>← Resources</a>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><a class="quiet-link" href={query("/history", { resource: id() })}>Search this resource's history →</a></Show>
    <Show when={result.data()} fallback={<Empty title={result.error() ? "Resource unavailable" : "Loading resource"} />}>{(detail) => <>
      <div class="page-heading resource-heading"><div><span class="eyebrow">{detail().resource.target.toUpperCase()} · {rule(detail().resource.check)}</span><h1>{detail().resource.id.split("/").at(-1)}</h1><p class="resource-path">{detail().resource.id}</p></div><Status health={stale(detail().resource.expires_at, dashboard.now()) ? "unknown" : detail().resource.health} /></div>
      <div class="detail-meta"><span>Observed {age(detail().resource.observed_at, dashboard.now())}</span><span>Expected: {rule(detail().resource.expected.replaceAll("_", "-"))}</span><a href={query("/history", { target: detail().resource.target, resource: detail().resource.id })}>View history →</a></div>
      <Show when={stale(detail().resource.expires_at, dashboard.now())}><Notice>This evidence is stale. Recovery requires a fresh observation.</Notice></Show>
      <section class="panel"><div class="panel-heading"><div><h2>Observed facts</h2><p>{utc(detail().resource.observed_at)}</p></div></div><dl class="facts-grid"><For each={detail().resource.facts}>{(fact) => <div><dt>{fact.label}</dt><dd>{fact.value}</dd></div>}</For></dl></section>
      <section class="panel"><div class="panel-heading"><h2>Related findings</h2><span class="count-label">{detail().findings.length} active</span></div><For each={detail().findings} fallback={<Empty title="No active findings for this resource" />} >{(finding) => <article class="finding-detail"><div><span class={`severity severity-${finding.severity}`}>{rule(finding.severity)}</span><h3>{rule(finding.rule)}</h3></div><p>{rule(finding.confidence)} evidence · {age(finding.observed_at, dashboard.now())}</p><div class="evidence-tags"><For each={finding.evidence}>{(reference) => <code>{reference}</code>}</For></div></article>}</For></section>
    </>}</Show>
  </>;
}

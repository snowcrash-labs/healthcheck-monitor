import { useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query, resourceId } from "./api";
import { resourceDetail } from "./schema";
import { ConsoleLinks, Location, Timestamp } from "./diagnostics";
import { ResourceEvidence, ResourceFindings, ResourceHistory } from "./resource-sections";
import { checkCatalog, checkPath, targetPath } from "./check-catalog";
import { rule, stale } from "./format";
import { Empty, Notice, Status } from "./components";

export default function DetailPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const params = useParams();
  const id = () => resourceId(params.id);
  const result = useQuery(() => query("/api/v1/resource", { id: id(), include_findings: "false" }), resourceDetail, dashboard.refreshId);
  return <>
    <a class="back-link" href={query("/resources", { target: dashboard.target() })}>← Resources</a>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><a class="quiet-link" href={query("/history", { resource: id() })}>Search this resource's history →</a></Show>
    <Show when={result.data()} fallback={<Empty title={result.error() ? "Resource unavailable" : "Loading resource"} />}>{(detail) => <>
      <div class="page-heading resource-heading"><div><span class="eyebrow"><a href={targetPath(detail().resource.target)}>{detail().resource.target}</a> · {rule(detail().resource.check)}</span><h1>{detail().resource.context?.name ?? detail().resource.id.split("/").at(-1)}</h1><p class="resource-path">{detail().resource.id}</p></div><Status health={stale(detail().resource.expires_at, dashboard.now()) ? "unknown" : detail().resource.health} /></div>
      <div class="detail-meta"><span>Latest observation <Timestamp at={detail().resource.observed_at} now={dashboard.now()} /></span><span>Expected: {rule(detail().resource.expected.replaceAll("_", "-"))}</span><a href={query("/history", { target: detail().resource.target, resource: detail().resource.id })}>View history →</a></div>
      <Show when={stale(detail().resource.expires_at, dashboard.now())}><Notice>This evidence is stale. Recovery requires a fresh observation.</Notice></Show>
      <ConsoleLinks links={detail().resource.links} />
      <ResourceFindings id={detail().resource.id} target={detail().resource.target} />
      <section class="panel"><div class="panel-heading"><h2>Resource location</h2></div><Location context={detail().resource.context} /></section>
      <nav class="detail-nav" aria-label="Contributing checks"><For each={detail().resource.checks}>{(check) => <a class="button" href={checkPath(detail().resource.target, check)}>{checkCatalog[check].title}</a>}</For></nav>
      <ResourceEvidence id={detail().resource.id} target={detail().resource.target} />
      <ResourceHistory id={detail().resource.id} target={detail().resource.target} />
    </>}</Show>
  </>;
}

import { For, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { query } from "./api";
import { useDashboard } from "./context";
import { resourceDetail } from "./schema";
import { useQuery } from "./use-query";
import { ConsoleLinks, Location, Timestamp } from "./diagnostics";
import { ResourceEvidence, ResourceFindings, ResourceHistory } from "./resource-sections";
import { checkCatalog, checkPath } from "./check-catalog";
import { locationText, resourceName, resourceType } from "./identity";
import { Empty, Notice, Status } from "./components";
import { ResourceCost } from "./resource-cost";
import { rule, stale } from "./format";

export function ResourceInspector(props: { id: string; panel?: boolean }) {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [search, setSearch] = useSearchParams();
  const tab = () => typeof search.tab === "string" && ["diagnostics", "history", "configuration", "cost"].includes(search.tab) ? search.tab : "summary";
  const result = useQuery(() => query("/api/v1/resource", { id: props.id, include_findings: "false" }), resourceDetail, dashboard.refreshId);
  return <div class="resource-inspector">
    <Show when={result.error()}><Notice error>{result.error()}</Notice><a href={query("/history", { resource: props.id })}>Search retained history</a></Show>
    <Show when={result.data()} fallback={<Empty title={result.error() ? "Resource unavailable" : "Loading resource"} />}>{(detail) => <>
      <header class="inspector-heading"><span class="muted">{resourceType(detail().resource.context)}</span>
        <Show when={props.panel} fallback={<h1>{resourceName(detail().resource.id, detail().resource.context)}</h1>}><h2>{resourceName(detail().resource.id, detail().resource.context)}</h2></Show>
        <p>{detail().resource.target} · {locationText(detail().resource.context)}</p>
        <Status health={stale(detail().resource.expires_at, dashboard.now()) ? "unknown" : detail().resource.health} />
        <ConsoleLinks links={detail().resource.links} />
      </header>
      <nav class="tabs" aria-label="Resource sections"><For each={["summary", "diagnostics", "history", "configuration", "cost"]}>{(section) => <button class="tab" aria-current={tab() === section ? "page" : undefined} onClick={() => setSearch({ tab: section === "summary" ? undefined : section, evidence: undefined, history: undefined })}>{rule(section)}</button>}</For></nav>
      <Show when={stale(detail().resource.expires_at, dashboard.now())}><Notice>Evidence is stale. Recovery needs a fresh observation.</Notice></Show>
      <Show when={tab() === "summary"}>
        <div class="detail-meta inspector-meta"><span>Last observed <Timestamp at={detail().resource.observed_at} now={dashboard.now()} /></span><span>Expected <strong>{rule(detail().resource.expected)}</strong></span></div>
        <ResourceFindings id={detail().resource.id} target={detail().resource.target} />
        <nav class="detail-nav" aria-label="Contributing checks"><For each={detail().resource.checks}>{(check) => <a class="button" href={checkPath(detail().resource.target, check)}>{checkCatalog[check].title}</a>}</For></nav>
      </Show>
      <Show when={tab() === "diagnostics"}><ResourceEvidence id={detail().resource.id} target={detail().resource.target} check="logs" /></Show>
      <Show when={tab() === "history"}><ResourceHistory id={detail().resource.id} target={detail().resource.target} /></Show>
      <Show when={tab() === "configuration"}>
        <section class="panel"><div class="panel-heading"><h2>Identity and location</h2></div><Location context={detail().resource.context} /><p class="panel-note resource-path">{detail().resource.id}</p><button class="button copy-identity" onClick={() => { void navigator.clipboard?.writeText(detail().resource.id).catch(() => undefined); }}>Copy resource ID</button></section>
        <ResourceEvidence id={detail().resource.id} target={detail().resource.target} />
      </Show>
      <Show when={tab() === "cost"}><ResourceCost resource={detail().resource} /></Show>
    </>}</Show>
  </div>;
}

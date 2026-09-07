import { useSearchParams } from "@solidjs/router";
import { For, Show, onSettled } from "solid-js";
import { useDashboard } from "./context";
import { usePages } from "./use-pages";
import { ScrollBoundary } from "./scroll-boundary";
import { query, resourceId, resourcePath } from "./api";
import { resourcePage } from "./schema";
import { findingTitle } from "./diagnostics";
import { locationText, resourceName, resourceType } from "./identity";
import { age, stale } from "./format";
import { Empty, Notice, Status } from "./components";
import { ResourceInspector } from "./resource-inspector";

export default function ResourcesPage() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const selected = () => resourceId(value("selected"));
  const result = usePages(() => query("/api/v1/resources", { target: dashboard.target(), q: value("q"), health: value("health"), check: value("check") }), resourcePage, dashboard.refreshId);
  let origin: HTMLElement | undefined;
  function close() { setParams({ selected: undefined, tab: undefined }); origin?.focus({ preventScroll: true }); }
  function open(event: MouseEvent, id: string) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0 || window.matchMedia("(max-width: 1100px)").matches) return;
    event.preventDefault(); origin = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
    setParams({ selected: resourcePath(id).split("/").at(-1), tab: undefined });
  }
  onSettled(() => {
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape" && selected()) close(); };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  });
  return <>
    <div class="page-heading"><div><h1>Resources</h1><p>{dashboard.target() || "All configured targets"} · Current observed state</p></div><span class="count-label">{result.data()?.total.toLocaleString() ?? "…"} resources</span></div>
    <div class="filters"><label>Search resources<input type="search" placeholder="Name, project, namespace, or identifier" value={value("q")} maxlength={128} onInput={(event) => setParams({ q: event.currentTarget.value || undefined, selected: undefined })} /></label><label>Health<select value={value("health")} onChange={(event) => setParams({ health: event.currentTarget.value || undefined, selected: undefined })}><option value="">All states</option><option value="unhealthy">Unhealthy</option><option value="degraded">Degraded</option><option value="unknown">Unknown</option><option value="healthy">Healthy</option><option value="expected_inactive">Expected inactive</option></select></label><Show when={value("q") || value("health") || value("check")}><button class="button" onClick={() => setParams({ q: undefined, health: undefined, check: undefined, selected: undefined })}>Clear filters</button></Show><span class="loading-label" aria-live="polite">{result.loading() ? "Updating…" : ""}</span></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={result.retry}>Retry</button></Show>
    <div class={selected() ? "investigation has-selection" : "investigation"}>
      <section class="panel resource-list" aria-busy={result.loading() ? "true" : "false"}><Show when={result.data()} fallback={<Empty title="Loading resources" />}>{(page) => <>
        <Show when={page().previous_cursor}><ScrollBoundary previous enabled loading={result.loading()} load={result.previous} /></Show>
        <Show when={page().items.length} fallback={<Empty title="No matching resources" detail="Try another scope, state, or search." />}><div class="table-scroll"><table class="resources-table"><thead><tr><th>Name / type</th><th>Location</th><th>Health</th><th>Primary problem</th><th>Last confirmed</th></tr></thead><tbody>
          <For each={page().items} keyed={(resource) => resource.id}>{(resource) => <tr data-row-key={resource().id} class={selected() === resource().id ? "selected" : undefined}>
            <td><a class="primary-link resource-name" href={resourcePath(resource().id, dashboard.target())} onClick={(event) => open(event, resource().id)} title={resource().id}>{resourceName(resource().id, resource().context)}</a><span class="cell-detail">{resourceType(resource().context)}</span></td>
            <td>{resource().target}<span class="cell-detail">{locationText(resource().context)}</span></td>
            <td><Status health={stale(resource().expires_at, dashboard.now()) ? "unknown" : resource().health} /></td>
            <td class="resource-findings"><Show when={resource().findings[0]} fallback={<span class="muted">No active problems</span>}>{(finding) => <><a class="primary-link" href={resourcePath(resource().id, dashboard.target())} onClick={(event) => open(event, resource().id)}>{findingTitle(finding().rule)}</a><Show when={finding().diagnostic?.facts[0]}>{(fact) => <span class="cell-detail">{fact().label}: {fact().value}</span>}</Show><Show when={resource().finding_count > 1}><span class="cell-detail">+{resource().finding_count - 1} more</span></Show></>}</Show></td>
            <td><time datetime={resource().observed_at}>{age(resource().observed_at, dashboard.now())}</time><Show when={stale(resource().expires_at, dashboard.now())}><span class="cell-detail text-warning">Stale</span></Show></td>
          </tr>}</For>
        </tbody></table></div></Show>
        <ScrollBoundary enabled={!!page().next_cursor} loading={result.loading()} total={page().total} load={result.next} />
      </>}</Show></section>
      <Show when={selected()}>{(id) => <aside class="inspector-panel" aria-label="Selected resource"><div class="inspector-actions"><a href={resourcePath(id(), dashboard.target())}>Open full page ↗</a><button class="button" aria-label="Close resource details" onClick={close}>Close ×</button></div><ResourceInspector id={id()} panel /></aside>}</Show>
    </div>
  </>;
}

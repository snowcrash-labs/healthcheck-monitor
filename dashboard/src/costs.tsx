import { For, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query } from "./api";
import { costView, contributorLabel, difference, money, units } from "./cost-schema";
import { CostChart } from "./cost-chart";
import { Empty, Notice, Pagination } from "./components";
import { age } from "./format";

export default function CostsPage() {
  const dashboard = useDashboard();
  const [params, setParams] = useSearchParams();
  const value = (key: string) => typeof params[key] === "string" ? params[key] : "";
  const set = (key: string, value: string) => setParams({ [key]: value || undefined, cursor: undefined, revision: undefined });
  const result = useQuery(() => query("/api/v1/query/costs/series", { target: dashboard?.target(), provider: value("provider"), scope: value("scope"), from: value("from"), to: value("to"), group: value("group"), granularity: value("granularity"), currency: value("currency"), contributor: value("contributor"), day: value("day"), q: value("q"), resource: value("resource"), cursor: value("cursor"), revision: value("revision") }), costView, () => dashboard?.costRefreshId() ?? 0);
  const select = (day: string, key: string) => setParams({ day: day || undefined, contributor: key === "__other__" ? undefined : key || undefined, cursor: undefined, revision: undefined });
  return <>
    <div class="page-heading"><div><h1>Costs</h1><p>Imported billing across connected services. Resource health does not affect these totals.</p></div></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice><button class="button" onClick={() => { setParams({ cursor: undefined, revision: undefined }); result.retry(); }}>Refresh cost selection</button></Show>
    <Show when={result.data()} fallback={<Empty title="Loading costs" />}>{(view) => <>
      <Show when={view().enabled} fallback={<Empty title="Billing is not connected" detail="Billing imports need an approved reader group and configured source. No estimated or synthetic charges are shown." />}>
        <div class="filters cost-filters">
          <label>From (UTC)<input type="date" value={value("from") || view().period.from} onChange={(e) => set("from", e.currentTarget.value)} /></label>
          <label>To (exclusive, UTC)<input type="date" value={value("to") || view().period.to} onChange={(e) => set("to", e.currentTarget.value)} /></label>
          <label>Provider<select value={value("provider")} onChange={(e) => set("provider", e.currentTarget.value)}><option value="">All connected</option><option value="gcp">Google Cloud</option><option value="aws">AWS</option><option value="azure">Azure</option></select></label><label>Group by<select value={value("group") || "provider"} onChange={(e) => { set("group", e.currentTarget.value); setParams({ contributor: undefined }); }}><option value="provider">Provider</option><option value="product">Cloud product</option><option value="scope">Project / account / subscription</option><option value="target">Environment</option><option value="region">Region</option><option value="category">Charge category</option><option value="resource">Resource</option></select></label>
          <label>Granularity<select value={value("granularity") || "daily"} onChange={(e) => set("granularity", e.currentTarget.value)}><option value="daily">Daily</option><option value="monthly">Monthly</option></select></label>
          <label>Currency<select value={value("currency") || "USD"} onChange={(e) => set("currency", e.currentTarget.value)}><option>USD</option><option>EUR</option><option>GBP</option><option>KRW</option><option>JPY</option></select></label>
        </div>
        <Show when={value("day") || value("contributor") || value("resource")}><div class="active-filters"><span>{[value("day"), contributorLabel(value("contributor")), value("resource")].filter(Boolean).join(" · ")}</span><button class="filter-chip" onClick={() => setParams({ day: undefined, contributor: undefined, resource: undefined, cursor: undefined, revision: undefined })}>Clear selection ×</button></div></Show>
        <section class="panel"><div class="cost-headline"><strong>{money(view().total, view().currency)}</strong><span>{difference(view().total, view().previous_total)}</span><small>Billed cost · Charge dates in UTC · Provisional until provider reconciliation</small></div>
          <Show when={view().series.length} fallback={<Empty title="No imported costs for this selection" detail="This is a coverage gap, not a zero-cost claim." />}><CostChart view={view()} select={select} compare={!!view().previous_total} /></Show>
        </section>
        <section class="panel"><div class="panel-heading"><h2>Cost breakdown</h2><label>Search contributors<input type="search" value={value("q")} maxlength={128} onInput={(e) => set("q", e.currentTarget.value)} /></label></div><div class="table-scroll"><table><thead><tr><th>Contributor</th><th class="numeric">Billed cost</th><th class="numeric">Share</th><th>Attribution</th></tr></thead><tbody><For each={view().breakdown} keyed={(row) => row.key}>{(row) => <tr><td><button class="text-button" onClick={() => select("", row().key)}>{contributorLabel(row().key)}</button></td><td class="numeric">{money(row().amount, view().currency)}</td><td class="numeric">{view().total && units(view().total ?? "0") > 0n && units(row().amount) >= 0n ? `${(Number(units(row().amount) * 1000n / units(view().total ?? "1")) / 10).toFixed(1)}%` : "Not applicable"}</td><td>{row().key ? "Provider export" : "Unallocated"}</td></tr>}</For></tbody></table></div><Pagination current={value("cursor")} next={view().next_cursor} total={view().contributor_count} onChange={(cursor) => setParams({ cursor: cursor || undefined, revision: cursor ? view().revision : undefined, from: view().period.from, to: view().period.to })} /></section>
      </Show>
      <section class="panel"><div class="panel-heading"><h2>Billing sources</h2><span class="muted">Monitoring access and billing coverage are separate</span></div><div class="table-scroll"><table><thead><tr><th>Provider</th><th>Import</th><th>Last received</th><th>Imported charge period</th></tr></thead><tbody><For each={["gcp", "aws", "azure"]}>{(provider) => { const sources = () => view().sources.filter((s) => s.provider === provider); return <For each={sources()} fallback={<tr><td>{contributorLabel(provider)}</td><td>Not connected</td><td>Not available</td><td>Not available</td></tr>}>{(source) => <tr><td>{contributorLabel(provider)}<span class="cell-detail">{source.id}</span></td><td>{source.fault || source.state}</td><td>{age(source.imported_at, dashboard?.now() ?? Date.now())}</td><td>{source.from && source.to ? `${source.from} to ${source.to} (exclusive)` : "Not yet imported"}</td></tr>}</For>; }}</For></tbody></table></div></section>
    </>}</Show>
  </>;
}

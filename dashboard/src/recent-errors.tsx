import { For, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { z } from "zod";
import { query, resourcePath } from "./api";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { ConsoleLinks, Timestamp } from "./diagnostics";
import { consoleLink } from "./schema";
import { Empty, Notice, Pagination } from "./components";
import { rule } from "./format";

const timestamp = z.iso.datetime({ offset: true });
const pageSchema = z.object({
  items: z.array(z.object({
    id: z.string(), resource: z.string().nullable(), observed_at: timestamp,
    scope: z.object({ target: z.string(), provider: z.string(), scope: z.string() }),
    location: z.object({ cluster: z.string().nullable(), namespace: z.string().nullable(), container: z.string().nullable() }),
    details: z.object({ kind: z.literal("diagnostic"), signature: z.string(), count: z.number().int().nonnegative(), first_seen: timestamp, last_seen: timestamp, window_start: timestamp, window_end: timestamp, sampled: z.boolean(), scanned: z.number().int().nonnegative(), complete: z.boolean(), gap_seconds: z.number().int().nonnegative(), links: z.array(consoleLink) }),
  })).max(100),
  next_cursor: z.string().nullable(),
  availability: z.object({ requested: z.object({ from: timestamp, to: timestamp }), complete: z.boolean(), gaps: z.array(z.string()) }),
});
export default function RecentErrors() {
  const dashboard = useDashboard();
  const [search, setSearch] = useSearchParams();
  const value = (key: string) => typeof search[key] === "string" ? search[key] : "";
  const result = useQuery(() => query("/api/v1/query/diagnostics", { target: dashboard?.target(), lookback_seconds: value("lookback") || "3600", from: value("from"), to: value("to"), q: value("q"), cluster: value("cluster"), namespace: value("namespace"), cursor: value("cursor") }), pageSchema, () => dashboard?.refreshId() ?? 0);
  return <><div class="page-heading"><div><h1>Problems</h1><p>Redacted errors sampled during the selected period.</p></div></div>
    <nav class="tabs" aria-label="Problem views"><a class="tab" href={query("/problems", { target: dashboard?.target() })}>Active</a><a class="tab" aria-current="page" href={query("/recent-errors", { target: dashboard?.target() })}>Recent errors</a><a class="tab" href={query("/history", { target: dashboard?.target() })}>Changes</a></nav>
    <div class="filters"><label>Search errors<input type="search" value={value("q")} maxlength={128} onInput={(e) => setSearch({ q: e.currentTarget.value || undefined, cursor: undefined })} /></label><label>Period<select value={value("lookback") || "3600"} onChange={(e) => setSearch({ lookback: e.currentTarget.value, from: undefined, to: undefined, cursor: undefined })}><option value="900">Last 15 minutes</option><option value="3600">Last hour</option><option value="86400">Last 24 hours</option></select></label></div>
    <Show when={result.error()}><Notice error>{result.error()}</Notice></Show>
    <Show when={result.data()} fallback={<Empty title="Loading diagnostics" />}>{(page) => <>
      <Show when={!page().availability.complete}><Notice>Diagnostic coverage is incomplete. Sample counts do not establish complete error rates.</Notice></Show>
      <section class="panel"><div class="panel-heading"><h2>Recent errors</h2><span class="muted">UTC · {page().availability.requested.from.slice(0, 16)} to {page().availability.requested.to.slice(0, 16)}</span></div>
        <For each={page().items} keyed={(row) => row.id} fallback={<Empty title="No retained error samples in this selection" detail="Missing or capped log windows cannot establish healthy silence." />}>{(row) => <article class="operation-detail" data-row-key={row().id}><h3>{rule(row().details.signature)} · {row().details.count} sampled occurrences</h3><p>{row().scope.target} · {row().scope.scope} · {[row().location.cluster, row().location.namespace, row().location.container].filter(Boolean).join(" / ")}</p><div class="detail-meta"><span>First seen <Timestamp at={row().details.first_seen} now={dashboard?.now() ?? Date.now()} /></span><span>Last seen <Timestamp at={row().details.last_seen} now={dashboard?.now() ?? Date.now()} /></span></div><p>{row().details.scanned} entries scanned · {row().details.complete ? "Window collected" : "Partial window"}</p><ConsoleLinks links={row().details.links} /><Show when={row().resource}>{(id) => <a href={resourcePath(id(), row().scope.target)}>Open recorded resource →</a>}</Show></article>}</For>
        <Pagination current={value("cursor")} next={page().next_cursor} onChange={(cursor) => setSearch({ cursor: cursor || undefined })} />
      </section>
    </>}</Show>
  </>;
}

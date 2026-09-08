import { Show } from "solid-js";
import type { Resource } from "./schema";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query } from "./api";
import { costView, money, coverageNotice } from "./cost-schema";
import { Empty, Notice } from "./components";

/** Resource costs require an exact native identifier; unallocated spend is never inferred. */
export function ResourceCost(props: { resource: Resource }) {
  const dashboard = useDashboard();
  const context = () => props.resource.context;
  const native = () => context()?.native_id.replace("https://www.googleapis.com/compute/v1/", "//compute.googleapis.com/") ?? "";
  return <Show when={context() && native()} fallback={<Empty title="Cost attribution unavailable" detail="This observation has no native resource identity." />}>{(id) => {
    const result = useQuery(() => query("/api/v1/query/costs/series", { scope: context()?.scope, resource: context()?.provider === "azure" ? id().toLowerCase() : id(), group: "resource" }), costView, () => dashboard?.costRefreshId() ?? 0);
    return <><Show when={result.error()}><Notice error>{result.error()}</Notice></Show><Show when={result.data()?.enabled && result.data()?.total !== null && result.data()} fallback={<Empty title="No attributable imported charges" detail="Costs may be unavailable, unallocated, or recorded under another native identifier." />}>{(view) => <section class="panel"><div class="cost-headline"><strong>{money(view().total, view().currency)}</strong><small>{view().period.from} to {view().period.to} · Charge dates in UTC</small></div><p class="panel-note">{coverageNotice(view())}</p></section>}</Show><a href={query("/costs", { scope: context()?.scope, resource: native(), group: "resource" })}>Explore resource costs →</a></>;
  }}</Show>;
}

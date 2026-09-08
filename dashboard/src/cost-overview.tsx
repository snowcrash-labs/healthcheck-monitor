import { Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { useDashboard } from "./context";
import { useQuery } from "./use-query";
import { query } from "./api";
import { costView, difference, money } from "./cost-schema";
import { CostChart } from "./cost-chart";
import { Empty } from "./components";

export function CostOverview() {
  const dashboard = useDashboard();
  const navigate = useNavigate();
  const result = useQuery(() => query("/api/v1/query/costs/series", { target: dashboard?.target() }), costView, () => dashboard?.costRefreshId() ?? 0);
  return <section class="panel cost-overview"><div class="panel-heading"><div><h2>Cloud cost</h2><p>{dashboard?.target() ? "Mapped to " + dashboard.target() : "All connected billing sources"}</p></div><a href={query("/costs", { target: dashboard?.target() })}>Explore costs →</a></div>
    <Show when={result.data()?.enabled && result.data()?.total !== null && result.data()} fallback={<Empty title={result.error() ? "Billing unavailable" : result.data()?.enabled ? "Waiting for billing data" : "Billing is not connected"} detail={result.error() || "No costs are inferred from resource counts. Imported billing data appears here when available."} />}>{(view) => <>
      <div class="cost-headline"><strong>{money(view().total, view().currency)}</strong><span>{difference(view().total, view().previous_total)}</span><small>{view().period.from} to {view().period.to} (exclusive) · Billed cost</small></div>
      <CostChart view={view()} compact select={(day, key) => { void navigate(query("/costs", { target: dashboard?.target(), day, contributor: key === "__other__" ? undefined : key })); }} />
    </>}</Show>
  </section>;
}

import type { CostView } from "../src/cost-schema";

/** Billing uses the same decimal range as PostgreSQL numeric(58,38). */
export const fractionalCosts: CostView = {
  enabled: true, revision: "fractional-usage", period: { from: "2026-09-01", to: "2026-09-07" },
  currency: "USD", measure: "billed", group: "provider", granularity: "daily",
  total: "12.340000732578337192535", previous_total: "10.010000732578337192535", complete: true,
  sources: [{ id: "azure", provider: "azure", state: "provisional", imported_at: "2026-09-07T00:00:00Z", from: "2026-08-01", to: "2026-09-07", revision: "fractional-usage", fault: null }],
  series: [{ date: "2026-09-06", total: "12.340000732578337192535", previous: "10.010000732578337192535", contributors: [{ key: "azure", amount: "12.340000732578337192535", previous: "10.010000732578337192535" }] }],
  breakdown: [{ key: "azure", amount: "12.340000732578337192535", previous: "10.010000732578337192535" }],
  next_cursor: null, contributor_count: 1,
};

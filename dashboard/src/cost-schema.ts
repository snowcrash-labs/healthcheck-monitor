import { z } from "zod";
export const amount = z.string().regex(/^-?\d{1,19}(?:\.\d{1,9})?$/);
const date = z.iso.date();
const contributor = z.object({ key: z.string(), amount, previous: amount.nullable() });
export const costView = z.object({
  enabled: z.boolean(), revision: z.string(), period: z.object({ from: date, to: date }),
  currency: z.string().regex(/^[A-Z]{3}$/), measure: z.enum(["billed", "effective"]),
  group: z.enum(["provider", "product", "scope", "region", "category", "resource", "target"]),
  granularity: z.enum(["daily", "monthly"]), total: amount.nullable(), previous_total: amount.nullable(),
  complete: z.boolean(),
  sources: z.array(z.object({ id: z.string(), provider: z.enum(["gcp", "aws", "azure", "external"]), state: z.string(), imported_at: z.iso.datetime({ offset: true }).nullable(), from: date.nullable(), to: date.nullable(), revision: z.string().nullable(), fault: z.string().nullable() })).max(16),
  series: z.array(z.object({ date, total: amount, previous: amount.nullable(), contributors: z.array(contributor).max(8) })).max(400),
  breakdown: z.array(contributor).max(100), next_cursor: z.string().nullable(), contributor_count: z.number().int().nonnegative(),
});
export type CostView = z.infer<typeof costView>;
export type CostPoint = CostView["series"][number];
/** Integer nanounits preserve exact comparisons; floating point is used only for SVG coordinates. */
export function units(value: string): bigint {
  const parsed = amount.safeParse(value);
  if (!parsed.success) return 0n;
  const negative = value.startsWith("-");
  const [whole = "0", fraction = ""] = value.replace(/^-/, "").split(".");
  const result = BigInt(whole) * 1_000_000_000n + BigInt(fraction.padEnd(9, "0"));
  return negative ? -result : result;
}
export function money(value: string | null, currency: string): string {
  if (value === null) return "Not available";
  const raw = units(value);
  const rounded = (raw < 0n ? -raw : raw) + 5_000_000n;
  const cents = rounded / 10_000_000n;
  const whole = (cents / 100n).toLocaleString("en-US");
  return `${raw < 0n ? "−" : ""}${currency} ${whole}.${(cents % 100n).toString().padStart(2, "0")}`;
}
export function difference(current: string | null, previous: string | null): string {
  if (current === null || previous === null) return "Comparison unavailable";
  const before = units(previous);
  if (before <= 0n) return "Percentage comparison unavailable";
  const change = Number((units(current) - before) * 1000n / before) / 10;
  return `${change > 0 ? "+" : ""}${change.toFixed(1)}% vs previous period`;
}
export function contributorLabel(value: string): string { return value === "__other__" ? "Other" : value === "" ? "Unallocated" : value === "gcp" ? "Google Cloud" : value === "aws" ? "AWS" : value === "azure" ? "Azure" : value; }
export function colorClass(value: string): string {
  if (value === "__other__") return "chart-other";
  const hash = [...value].reduce((sum, c) => (sum * 31 + c.charCodeAt(0)) >>> 0, 0);
  return `chart-${hash % 5 + 1}`;
}

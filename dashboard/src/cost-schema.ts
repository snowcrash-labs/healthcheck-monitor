import { z } from "zod";
export const amount = z.string().regex(/^-?\d{1,20}(?:\.\d{1,38})?$/);
const decimalPlaces = 38;
const decimalUnits = 10n ** BigInt(decimalPlaces);
const centUnits = decimalUnits / 100n;
const date = z.iso.date();
/** `amount` is list-price spend; `credits` is the signed adjustment (zero or negative) applied to it. */
const contributor = z.object({ key: z.string(), amount, credits: amount, previous: amount.nullable() });
export const costView = z.object({
  enabled: z.boolean(), revision: z.string(), period: z.object({ from: date, to: date }),
  currency: z.string().regex(/^[A-Z]{3}$/), measure: z.enum(["billed", "effective"]),
  group: z.enum(["provider", "product", "scope", "region", "category", "resource", "target"]),
  granularity: z.enum(["daily", "monthly"]), total: amount.nullable(), credits: amount.nullable(), previous_total: amount.nullable(),
  complete: z.boolean(),
  sources: z.array(z.object({ id: z.string(), provider: z.enum(["gcp", "aws", "azure", "external"]), state: z.string(), imported_at: z.iso.datetime({ offset: true }).nullable(), from: date.nullable(), to: date.nullable(), revision: z.string().nullable(), fault: z.string().nullable() })).max(16),
  series: z.array(z.object({ date, total: amount, credits: amount, previous: amount.nullable(), contributors: z.array(contributor).max(8) })).max(400),
  breakdown: z.array(contributor).max(100), next_cursor: z.string().nullable(), contributor_count: z.number().int().nonnegative(),
});
export type CostView = z.infer<typeof costView>;
export type CostPoint = CostView["series"][number];
/** Integer decimal units preserve exact comparisons; floating point is used only for SVG coordinates. */
export function units(value: string): bigint {
  const parsed = amount.safeParse(value);
  if (!parsed.success) return 0n;
  const negative = value.startsWith("-");
  const [whole = "0", fraction = ""] = value.replace(/^-/, "").split(".");
  const result = BigInt(whole) * decimalUnits + BigInt(fraction.padEnd(decimalPlaces, "0"));
  return negative ? -result : result;
}
/** Render integer decimal units back to the backend's plain decimal string. */
export function plain(value: bigint): string {
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  const fraction = (magnitude % decimalUnits).toString().padStart(decimalPlaces, "0").replace(/0+$/, "");
  return `${negative ? "-" : ""}${magnitude / decimalUnits}${fraction ? "." + fraction : ""}`;
}
/** Spend after credits; both inputs are exact decimal strings so no float rounding occurs. */
export function net(amount: string | null, credits: string | null): string | null {
  if (amount === null || credits === null) return null;
  return plain(units(amount) + units(credits));
}
export function hasCredits(credits: string | null | undefined): credits is string {
  return typeof credits === "string" && units(credits) !== 0n;
}
export function money(value: string | null, currency: string): string {
  if (value === null) return "Not available";
  const raw = units(value);
  const rounded = (raw < 0n ? -raw : raw) + centUnits / 2n;
  const cents = rounded / centUnits;
  const whole = (cents / 100n).toLocaleString("en-US");
  return `${raw < 0n ? "−" : ""}${currency} ${whole}.${(cents % 100n).toString().padStart(2, "0")}`;
}
export function difference(current: string | null, previous: string | null, compact = false): string {
  if (current === null || previous === null) return compact ? "Not available" : "Comparison unavailable";
  const before = units(previous);
  if (before <= 0n) return compact ? "Not applicable" : "Percentage comparison unavailable";
  const change = Number((units(current) - before) * 1000n / before) / 10;
  return `${change > 0 ? "+" : ""}${change.toFixed(1)}%${compact ? "" : " vs previous period"}`;
}
export function contributorLabel(value: string): string { if (value.startsWith("v:")) value = value.slice(2); return value === "__other__" ? "Other" : value === "" ? "Unallocated" : value === "gcp" ? "Google Cloud" : value === "aws" ? "AWS" : value === "azure" ? "Azure" : value; }
export function colorClass(value: string): string {
  if (value === "__other__") return "chart-other";
  const hash = [...value].reduce((sum, c) => (sum * 31 + c.charCodeAt(0)) >>> 0, 0);
  return `chart-${hash % 5 + 1}`;
}

export function coverageNotice(view: CostView): string {
  const unavailable = view.sources.filter((source) => !source.imported_at || source.fault || source.state === "stale");
  if (unavailable.length) return "Some billing sources are unavailable. Totals include only the retained charges.";
  const starts = view.sources.flatMap((source) => source.from ? [source.from] : []).sort();
  const latestStart = starts.at(-1);
  if (latestStart && latestStart > view.period.from) return `Partial period: imported coverage begins ${latestStart} for at least one source.`;
  return "Provisional provider charges before credits; late adjustments may change totals.";
}

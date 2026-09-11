import { describe, it, expect } from "vitest";
import { amount, costView, units, money, difference, colorClass, net, plain, hasCredits } from "../../src/cost-schema";
import { fractionalCosts } from "../cost-fixture";
describe("billing amounts", () => {
  it("rejects non-decimal, oversized, and lossy values", () => {
    for (const value of ["NaN", "1e5", `1.${"0".repeat(38)}1`, "100000000000000000000", "customer payload"]) expect(amount.safeParse(value).success).toBe(false);
    expect(amount.safeParse("-0.123456789").success).toBe(true);
  });
  it("accepts the backend decimal contract across the entire billing view", () => {
    expect(costView.safeParse(fractionalCosts).success).toBe(true);
    const smallest = `0.${"0".repeat(37)}1`;
    expect(amount.safeParse(smallest).success).toBe(true);
    expect(units(smallest)).toBe(1n);
    expect(units(`-${smallest}`)).toBe(-1n);
    expect(units("1.00000000000000000000000000000000000001") - units("1")).toBe(1n);
    expect(money("12.340000732578337192535", "USD")).toBe("USD 12.34");
    expect(money("0.00499999999999999999999999999999999999", "USD")).toBe("USD 0.00");
    expect(money("0.00500000000000000000000000000000000001", "USD")).toBe("USD 0.01");
    expect(difference(`0.${"0".repeat(37)}2`, smallest)).toBe("+100.0% vs previous period");
  });
  it("keeps nanounits exact and rounds signed display values", () => {
    expect(units("0.1") + units("0.2")).toBe(units("0.3"));
    expect(money("-1.255", "USD")).toBe("−USD 1.26");
    expect(money("12345678901234567.12", "USD")).toBe("USD 12,345,678,901,234,567.12");
    expect(money(null, "USD")).toBe("Not available");
  });
  it("does not imply a percentage change from missing or negative baselines", () => {
    expect(difference("10", null)).toBe("Comparison unavailable");
    expect(difference("10", "0")).toBe("Percentage comparison unavailable");
    expect(difference("10", "-5")).toBe("Percentage comparison unavailable");
    expect(difference("110", "100")).toBe("+10.0% vs previous period");
    expect(difference("110", "100", true)).toBe("+10.0%");
    expect(difference("10", null, true)).toBe("Not available");
    expect(difference("10", "0", true)).toBe("Not applicable");
  });
  it("derives net spend from exact credits without float drift", () => {
    expect(plain(units("12.340000732578337192535"))).toBe("12.340000732578337192535");
    expect(plain(0n)).toBe("0");
    expect(plain(-1n)).toBe(`-0.${"0".repeat(37)}1`);
    expect(net("1042.73", "-1042.73")).toBe("0");
    expect(net("0.3", "-0.1")).toBe("0.2");
    expect(net("10", null)).toBeNull();
    expect(hasCredits("0")).toBe(false);
    expect(hasCredits("-0.000000000000000000000000000000000001")).toBe(true);
    expect(hasCredits(null)).toBe(false);
    expect(costView.parse(fractionalCosts).credits).toBe("-2.000000000000000000000001");
  });
  it("keeps contributor colors stable across reordering", () => {
    expect(colorClass("Compute")).toBe(colorClass("Compute"));
    expect(colorClass("__other__")).toBe("chart-other");
  });
});

import { describe, expect, it } from "vitest";
import { checkCatalog, checkState, checkStateText, coverageText, interval } from "../../src/check-catalog";
import { check, checkView, consoleLink, coverage } from "../../src/schema";
import { rule } from "../../src/format";

describe("operator descriptions", () => {
  it("describes every check and coverage outcome without internal enum formatting", () => {
    for (const kind of check.options) expect(checkCatalog[kind].description.length).toBeGreaterThan(30);
    for (const kind of coverage.options) {
      expect(coverageText[kind].title).not.toContain("_");
      expect(coverageText[kind].detail.length).toBeGreaterThan(20);
    }
    expect(rule("inventory_only")).toBe("Inventory only");
    expect(interval(300)).toBe("5 minutes");
  });
  it("distinguishes waiting, stale, and incomplete evidence from complete required coverage", () => {
    const now = Date.now();
    const row = checkView.parse({ key: "dev/releases", target: "dev", check: "releases", interval_seconds: 300, started_at: null, finished_at: null, expires_at: null, complete: true, observations: 0, required_failures: 0, optional_gaps: 2, prerequisite: false, failures: [] });
    expect(checkState(row, now)).toBe("awaiting");
    row.finished_at = new Date(now).toISOString(); row.expires_at = new Date(now + 300_000).toISOString();
    expect(checkStateText[checkState(row, now)]).toBe("Complete required evidence");
    row.complete = false;
    expect(checkState(row, now)).toBe("incomplete");
    expect(checkState(row, now + 301_000)).toBe("stale");
  });
  it("rejects arbitrary console destinations", () => {
    for (const url of ["javascript:alert(1)", "https://console.cloud.google.com.evil.test/", "https://evil.test/", "http://portal.azure.com/"]) {
      expect(consoleLink.safeParse({ label: "Console", url }).success).toBe(false);
    }
    for (const url of ["https://console.cloud.google.com/run", "https://us-west-2.console.aws.amazon.com/", "https://portal.azure.com/#resource/subscriptions/example"]) {
      expect(consoleLink.safeParse({ label: "Console", url }).success).toBe(true);
    }
  });
});

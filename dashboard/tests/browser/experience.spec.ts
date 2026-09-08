import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { fixture } from "./fixtures";

async function billing(page: Page, withOther = false) {
  const points = [
    { date: "2026-09-05", total: "110", previous: "100", contributors: [{ key: "gcp", amount: "100", previous: null }, { key: "aws", amount: "10", previous: null }] },
    { date: "2026-09-06", total: "90", previous: "80", contributors: [{ key: "gcp", amount: "100", previous: null }, { key: "aws", amount: "-10", previous: null }] },
  ];
  if (withOther) for (const point of points) point.contributors.push({ key: "__other__", amount: "5", previous: null });
  await page.route("**/api/v1/query/costs/series**", (route) => {
    const url = new URL(route.request().url()); const day = url.searchParams.get("day"); const key = url.searchParams.get("contributor");
    const series = points.filter((p) => !day || p.date === day).map((p) => {
      const contributors = p.contributors.filter((c) => !key || c.key === key).map((c) => key === "__other__" ? { ...c, key: "v:" } : c);
      return { ...p, contributors, total: String(contributors.reduce((sum, c) => sum + Number(c.amount), 0)) };
    });
    const totals = new Map<string, number>();
    for (const point of series) for (const row of point.contributors) totals.set(row.key, (totals.get(row.key) ?? 0) + Number(row.amount));
    return route.fulfill({ json: { enabled: true, revision: "synthetic", period: { from: "2026-09-01", to: "2026-09-07" }, currency: "USD", measure: "billed", group: "provider", granularity: "daily", total: String(series.reduce((sum, p) => sum + Number(p.total), 0)), previous_total: "180", complete: false, sources: ["gcp", "aws"].map((provider) => ({ id: provider, provider, state: "provisional", imported_at: "2026-09-07T00:00:00Z", from: "2026-08-01", to: "2026-09-07", revision: "synthetic", fault: null })), series, breakdown: [...totals].map(([key, amount]) => ({ key, amount: String(amount), previous: null })), next_cursor: null, contributor_count: totals.size } });
  });
}

test("overview puts problems and actual API billing above the target list", async ({ page }, info) => {
  const diagnostics = await fixture(page); await billing(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await expect(page.getByText("USD 200.00", { exact: true }).first()).toBeVisible();
  const chart = await page.locator(".cost-chart").boundingBox();
  expect(chart && chart.y + chart.height).toBeLessThan(900);
  await expect(page.locator(".check-grid")).toHaveCount(0);
  await page.screenshot({ path: info.outputPath("overview.png"), fullPage: true });
  diagnostics.assertReactiveDiagnostics();
});

test("Other opens its remaining contributors and labels missing attribution", async ({ page }) => {
  const diagnostics = await fixture(page); await billing(page, true); await page.goto("/costs");
  await page.locator(".chart-legend").getByRole("button", { name: "Other", exact: true }).click();
  await expect(page).toHaveURL(/contributor=__other__/);
  await expect(page).toHaveURL(/from=2026-09-01/);
  await expect(page.getByText("USD 10.00", { exact: true }).first()).toBeVisible();
  const breakdown = page.locator("section").filter({ has: page.getByRole("heading", { name: "Cost breakdown" }) });
  await expect(breakdown.getByRole("button", { name: "Unallocated", exact: true })).toBeVisible();
  await expect(breakdown.getByRole("cell", { name: "Unallocated", exact: true }).last()).toBeVisible();
  await expect(breakdown.getByRole("columnheader", { name: "Change", exact: true })).toBeVisible();
  diagnostics.assertReactiveDiagnostics();
});

test("cost chart selection filters the table and back restores the range", async ({ page }, info) => {
  const diagnostics = await fixture(page); await billing(page); await page.goto("/costs");
  await expect(page.getByText("USD 200.00", { exact: true }).first()).toBeVisible();
  await expect(page.locator(".chart-bar")).toHaveCount(4);
  await page.locator(".chart-bar").first().focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/day=2026-09-05/);
  await expect(page.getByText("USD 100.00", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear selection" })).toBeVisible();
  await page.goBack();
  await expect(page.locator(".chart-bar")).toHaveCount(4);
  await page.getByText("View accessible daily cost table").click();
  await expect(page.getByRole("button", { name: "2026-09-05", exact: true })).toBeVisible();
  await page.getByLabel("Appearance", { exact: true }).selectOption("dark");
  await page.screenshot({ path: info.outputPath("costs-dark.png"), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(390);
  diagnostics.assertReactiveDiagnostics();
});

test("resource preview, tab, row identity, and keyboard focus survive refreshes", async ({ page }) => {
  await fixture(page);
  const at = new Date().toISOString(); const expires = new Date(Date.now() + 3600_000).toISOString();
  let generation = 1;
  await page.route("**/api/v1/resources?**", (route) => route.fulfill({ json: { generation: generation++, items: [{ id: "dev/endpoints/api", target: "dev", check: "edge", health: "unhealthy", expected: "active", observed_at: at, expires_at: expires, facts: [], finding_count: 0, findings: [] }], next_cursor: null, previous_cursor: null, total: 1 } }));
  await page.setViewportSize({ width: 1440, height: 900 }); await page.goto("/resources");
  const row = page.locator(".resources-table tbody tr").first();
  await expect(row).toBeVisible(); const original = await row.elementHandle();
  await row.getByRole("link").first().click();
  await expect(page).toHaveURL(/selected=/);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.getByRole("searchbox", { name: "Search evidence" }).fill("retained");
  await page.getByRole("button", { name: "Refresh view", exact: true }).click();
  await expect(page.getByRole("searchbox", { name: "Search evidence" })).toHaveValue("retained");
  expect(await original?.evaluate((node) => node.isConnected)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.locator(".inspector-panel")).toHaveCount(0);
  await expect(row.getByRole("link").first()).toBeFocused();
});

test("revoked authorization removes previously loaded resource data", async ({ page }) => {
  await fixture(page);
  let denied = false;
  await page.route("**/api/v1/resources?**", (route) => route.fulfill(denied ? { status: 401 } : { json: { generation: 1, items: [{ id: "dev/private-resource", target: "dev", check: "inventory", health: "healthy", expected: "active", observed_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3600_000).toISOString(), facts: [], finding_count: 0, findings: [] }], next_cursor: null, previous_cursor: null, total: 1 } }));
  await page.goto("/resources");
  await expect(page.getByText("private-resource", { exact: true })).toBeVisible();
  denied = true;
  await page.getByRole("button", { name: "Refresh view", exact: true }).click();
  await expect(page.getByText("Your sign-in expired", { exact: false })).toBeVisible();
  await expect(page.getByText("private-resource", { exact: true })).toHaveCount(0);
});

test("a minute of revisions preserves resource rows and a paused view", async ({ page }) => {
  await page.clock.install();
  const diagnostics = await fixture(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/resources");
  const row = page.locator(".resources-table tbody tr").first();
  await expect(row).toBeVisible();
  const original = await row.elementHandle();
  await row.getByRole("link").first().click();
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.getByRole("searchbox", { name: "Search evidence" }).fill("kept");
  for (let tick = 0; tick < 6; tick++) {
    await page.clock.runFor(10000);
    expect(await original?.evaluate((node) => node.isConnected)).toBe(true);
    await expect(page.getByRole("searchbox", { name: "Search evidence" })).toHaveValue("kept");
  }
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  const paused = await page.getByText("Loaded view paused", { exact: false }).textContent();
  await page.clock.runFor(60000);
  await expect(page.getByText("Loaded view paused", { exact: false })).toHaveText(paused ?? "");
  await page.getByRole("button", { name: "Resume live", exact: true }).click();
  await expect(page.getByText("Loaded view paused", { exact: false })).toHaveCount(0);
  diagnostics.assertReactiveDiagnostics();
});

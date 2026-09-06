import { expect, test } from "@playwright/test";
import { z } from "zod";

test("infinite scrolling evicts and reloads pages while search covers all results", async ({ page }) => {
  const rows = Array.from({ length: 400 }, (_, index) => ({
    id: `fixture/rows/${String(index).padStart(4, "0")}`, target: "fixture", check: "inventory", health: "healthy", expected: "active",
    observed_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3600_000).toISOString(),
    facts: [{ label: "State", value: "Running" }], finding_count: 0, findings: [],
  }));
  await page.route(/\/api\/v1\/resources\?/, async (route) => {
    const url = new URL(route.request().url());
    const q = url.searchParams.get("q") ?? "";
    const selected = rows.filter((row) => row.id.includes(q));
    const cursor = url.searchParams.get("cursor");
    let start = 0; let end = 50;
    if (cursor) {
      const raw: unknown = JSON.parse(cursor);
      const boundary = z.object({ resource: z.string() }).parse(raw);
      const index = selected.findIndex((row) => row.id >= boundary.resource);
      if (url.searchParams.get("direction") === "previous") { end = Math.max(0, index); start = Math.max(0, end - 50); }
      else { start = index < 0 ? selected.length : index + 1; end = start + 50; }
    }
    const items = selected.slice(start, end);
    const last = items.at(-1);
    const encode = (resource: string) => JSON.stringify({ priority: 0, resource, identity: "" });
    await route.fulfill({ json: {
      generation: 1, items, total: selected.length,
      next_cursor: end < selected.length && last ? encode(last.id) : null,
      previous_cursor: start > 0 && items[0] ? encode(items[0].id) : null,
    } });
  });
  await page.goto("/resources");
  await expect(page.locator("tbody tr")).toHaveCount(50);
  for (let index = 1; index < 8; index++) {
    await page.locator(".scroll-boundary").last().scrollIntoViewIfNeeded();
    await expect(page.locator("tbody tr").last()).toHaveAttribute("data-row-key", `fixture/rows/${String((index + 1) * 50 - 1).padStart(4, "0")}`);
    expect(await page.locator("tbody tr").count()).toBeLessThanOrEqual(150);
  }
  await expect(page.getByRole("button", { name: "All results reached" })).toBeVisible();
  await page.locator(".scroll-boundary").first().scrollIntoViewIfNeeded();
  await expect(page.locator("tbody tr").first()).toHaveAttribute("data-row-key", "fixture/rows/0200");
  await page.getByRole("searchbox").fill("0005");
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await expect(page.locator("tbody tr")).toHaveAttribute("data-row-key", "fixture/rows/0005");
});

test("failed initial reads can be retried", async ({ page }) => {
  let unavailable = true;
  await page.route(/\/api\/v1\/resources\?/, async (route) => {
    if (unavailable) { await route.fulfill({ status: 503 }); return; }
    await route.fulfill({ json: { generation: 1, items: [], total: 0, next_cursor: null, previous_cursor: null } });
  });
  await page.goto("/resources");
  await expect(page.getByRole("alert")).toContainText("temporarily unavailable");
  unavailable = false;
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("heading", { name: "No matching resources" })).toBeVisible();
});

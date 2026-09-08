import { expect, test } from "@playwright/test";
import { fixture } from "./fixtures";
import { fractionalCosts } from "../cost-fixture";
import { readFileSync, statSync } from "node:fs";
import { costView, money } from "../../src/cost-schema";

test("fractional cloud charges render in the overview and billing explorer", async ({ page }) => {
  const diagnostics = await fixture(page);
  await page.route("**/api/v1/query/costs/series**", (route) => route.fulfill({ json: fractionalCosts }));
  for (const path of ["/", "/costs"]) {
    await page.goto(path);
    await expect(page.getByText("USD 12.34", { exact: true }).first()).toBeVisible();
    await expect(page.getByText("The dashboard received an invalid update.", { exact: true })).toHaveCount(0);
    await expect(page.locator(".chart-bar")).toHaveCount(1);
  }
  await page.goto("/resources");
  await page.locator(".resources-table tbody tr").first().getByRole("link").first().click();
  await page.getByRole("button", { name: "Cost", exact: true }).click();
  await expect(page.locator(".inspector-panel .cost-headline strong")).toHaveText("USD 12.34");
  diagnostics.assertReactiveDiagnostics();
});

test("captured production billing renders through the frontend contract", async ({ page }, info) => {
  const path = process.env.HEALTHCHECK_BILLING_RESPONSE_FILE;
  test.skip(!path, "Requires an explicit protected billing response capture");
  if (!path || statSync(path).size > 2 * 1024 * 1024) throw new Error("Invalid billing capture");
  const raw: unknown = JSON.parse(readFileSync(path, "utf8"));
  const view = costView.parse(raw);
  const diagnostics = await fixture(page);
  await page.route("**/api/v1/query/costs/series**", (route) => route.fulfill({ json: view }));
  await page.goto("/costs");
  await expect(page.locator(".cost-headline strong")).toHaveText(money(view.total, view.currency));
  await expect(page.getByText("The dashboard received an invalid update.", { exact: true })).toHaveCount(0);
  await expect(page.locator(".chart-bar:visible").first()).toBeVisible();
  await page.screenshot({ path: info.outputPath("billing-capture.png"), fullPage: true });
  diagnostics.assertReactiveDiagnostics();
});

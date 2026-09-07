import { expect, test } from "@playwright/test";
import { resourcePath } from "../../src/api";
import { fixture } from "./fixtures";

test("target panels open scoped overviews and expose every configured check", async ({ page }) => {
  await fixture(page);
  await page.goto("/");
  await page.getByRole("link", { name: "dev", exact: true }).click();
  await expect(page).toHaveURL(/\/targets\/dev/);
  await expect(page.getByRole("heading", { name: "dev", exact: true })).toBeVisible();
  await expect(page.getByLabel("Monitoring target")).toHaveValue("dev");
  await page.getByRole("link", { name: /11 of 12 checks/ }).click();
  await expect(page.getByRole("heading", { name: "Checks", exact: true })).toBeVisible();
  await expect(page.locator(".checks-table tbody tr")).toHaveCount(12);
  await expect(page.getByText("Every 5 minutes", { exact: false }).first()).toBeVisible();
  await expect(page.getByText("Inventory_only", { exact: false })).toHaveCount(0);
  await page.getByRole("link", { name: "Release verification", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Collection operations" })).toBeVisible();
  await expect(page.getByText("The collector did not record which cutoff was reached.", { exact: false }).first()).toBeVisible();
  await page.getByRole("searchbox", { name: "Search operations" }).fill("0136");
  await expect(page.getByRole("heading", { name: "Operation 0136" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "Release verification" })).toBeVisible();
  await page.goBack();
  await expect(page.getByLabel("Monitoring target")).toHaveValue("dev");
});

test("resource problems show what when where and retain multiple evidence sources", async ({ page }, testInfo) => {
  await fixture(page);
  await page.goto(resourcePath("dev/endpoints/api", "dev"));
  await expect(page.getByRole("heading", { name: "Endpoint failed its DNS, TLS, or HTTP check" })).toBeVisible();
  await expect(page.getByText("First detected", { exact: false })).toBeVisible();
  await expect(page.getByText("Latest confirmed failure", { exact: false })).toBeVisible();
  await expect(page.getByText("503", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("Accepted HTTP statuses", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("dev · example · us-central1", { exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "Open in Google Cloud" }).first()).toHaveAttribute("href", /run\/detail\/us-central1\/api\/metrics\?project=example/);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await expect(page.locator("[data-row-key='edge/endpoints']")).toBeVisible();
  await expect(page.locator("[data-row-key='inventory/service']")).toBeVisible();
  await page.getByLabel("Appearance", { exact: true }).selectOption("dark");
  await page.screenshot({ path: testInfo.outputPath("resource-diagnostics-dark.png"), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(390);
  await page.getByLabel("Appearance", { exact: true }).selectOption("light");
  await page.screenshot({ path: testInfo.outputPath("resource-diagnostics-mobile.png"), fullPage: true });
});

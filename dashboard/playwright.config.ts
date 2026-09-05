import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests/browser",
  workers: 1,
  retries: 0,
  timeout: 30_000,
  use: { baseURL: process.env.HEALTHCHECK_DASHBOARD_URL ?? "http://127.0.0.1:9840", ignoreHTTPSErrors: true, headless: true, screenshot: "only-on-failure", trace: "retain-on-failure" },
});

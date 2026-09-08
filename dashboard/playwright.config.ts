import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests/browser",
  workers: 1,
  retries: 0,
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "firefox", use: { browserName: "firefox" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  timeout: 30_000,
  ...(process.env.HEALTHCHECK_DASHBOARD_URL ? {} : { webServer: { command: "npm run dev -- --port 9841 --strictPort", url: "http://127.0.0.1:9841", reuseExistingServer: !process.env.CI } }),
  use: { baseURL: process.env.HEALTHCHECK_DASHBOARD_URL ?? "http://127.0.0.1:9841", ignoreHTTPSErrors: true, headless: true, screenshot: "only-on-failure", trace: "retain-on-failure" },
});

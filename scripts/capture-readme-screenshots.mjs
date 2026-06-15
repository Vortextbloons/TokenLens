import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(__dirname, "..", "docs", "screenshots");
const baseUrl = process.env.SCREENSHOT_BASE_URL ?? "http://localhost:5173";

const pages = [
  { file: "overview-dark.png", hash: "#/" },
  { file: "sessions-dark.png", hash: "#/sessions" },
  { file: "session-detail-dark.png", hash: "#/sessions/1" },
  { file: "models-dark.png", hash: "#/models" },
  { file: "providers-dark.png", hash: "#/providers" },
  { file: "projects-dark.png", hash: "#/projects" },
  { file: "costs-dark.png", hash: "#/costs" },
  { file: "timeline-dark.png", hash: "#/timeline" },
  { file: "raw-events-dark.png", hash: "#/raw-events" },
  { file: "settings-dark.png", hash: "#/settings" },
];

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
const context = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
  colorScheme: "dark",
});

await context.addInitScript(() => {
  localStorage.setItem(
    "tokenlens-theme",
    JSON.stringify({ state: { theme: "dark", resolved: "dark" }, version: 0 })
  );
  document.documentElement.classList.add("dark");
});

const page = await context.newPage();

for (const { file, hash } of pages) {
  await page.goto(`${baseUrl}/${hash}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(800);
  await page.evaluate(() => {
    for (const span of document.querySelectorAll("span")) {
      if (span.textContent === "Mock") {
        const row = span.closest(".hidden");
        if (row) row.remove();
      }
    }
  });
  await page.screenshot({
    path: path.join(outDir, file),
    fullPage: false,
  });
  console.log(`wrote ${file}`);
}

await browser.close();

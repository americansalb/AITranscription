// Visual check: real Chromium screenshot of ui2 with mocked board data.
// Run: node e2e/screenshot.mjs [url] [out.png]   (from desktop/)
import { chromium } from "@playwright/test";
import { build } from "esbuild";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const url = process.argv[2] || "http://localhost:1420/#/ui2";
const out = process.argv[3] || resolve(here, "ui2-screenshot.png");

const bundle = await build({
  entryPoints: [resolve(here, "tauriMock.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "neutral",
});
const mod = await import(
  "data:text/javascript;base64," + Buffer.from(bundle.outputFiles[0].text).toString("base64")
);

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
await page.addInitScript(mod.tauriMockSource(300));
await page.goto(url);
await page.waitForSelector(".ui2-row", { timeout: 15000 });
await page.waitForTimeout(500);
await page.screenshot({ path: out, fullPage: false });
await browser.close();
console.log("screenshot written:", out);

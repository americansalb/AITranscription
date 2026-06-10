// Visual evidence run (human msg 439: "are you even taking screenshots").
// Captures ui2 under both OS color schemes against the production bundle —
// proves what the surface actually renders, not what the tokens intend.
// Run: npx playwright test e2e/screenshot.spec.ts
import { test } from "@playwright/test";
import { tauriMockSource } from "./tauriMock";

for (const scheme of ["dark", "light"] as const) {
  test(`screenshot ui2 — OS ${scheme} mode`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.addInitScript(tauriMockSource(300));
    await page.goto("/#/ui2");
    await page.locator(".ui2-row").first().waitFor();
    await page.screenshot({ path: `e2e/screenshots/ui2-os-${scheme}.png`, fullPage: false });
  });
}

// ui2 e2e + paint-side perf (register ②③) — real Chromium, real paint
// pipeline, mocked Tauri IPC (see tauriMock.ts header for the limitation).
// §7 bars measured here: 5k-board initial render < 1s · no long tasks while
// typing (proxy for keystroke→paint < 16ms) · timeline scroll ≥ ~55fps.
import { expect, test, type Page } from "@playwright/test";
import { tauriMockSource } from "./tauriMock";

declare global {
  interface Window {
    __UI2_SENT: Array<{ cmd: string; args: Record<string, unknown> }>;
    __TAURI_INTERNALS__: { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
  }
}

async function openUi2(page: Page, messageCount: number) {
  await page.addInitScript(tauriMockSource(messageCount));
  await page.goto("/#/ui2");
}

test("five-step smoke: launch → feed → card → choose → mute (real browser)", async ({ page }) => {
  await openUi2(page, 300);

  // 1. launch
  await expect(page.getByText("E2EProject")).toBeVisible();

  // 2. feed: relay rows expanded, engine noise collapsed into digests
  await expect(page.locator(".ui2-msg").first()).toBeVisible();
  await expect(page.locator(".ui2-digest").first()).toBeVisible();
  await expect(page.getByText("body body", { exact: false }).first()).toBeVisible();

  // 3. card active in the dock
  const option = page.getByRole("button", { name: "Approve and continue" });
  await expect(option).toBeVisible();

  // 4. choose: resolution lands on the (mock) board with in_reply_to
  await option.click();
  await expect
    .poll(async () =>
      page.evaluate(() => window.__UI2_SENT.filter((s) => s.cmd === "send_team_message").length),
    )
    .toBeGreaterThan(0);
  const resolved = await page.evaluate(() => window.__UI2_SENT[0].args.metadata);
  expect(resolved).toMatchObject({ choice_id: "a" });

  // 5. mute: pressed state + standing directive posted
  await page.getByRole("button", { name: "Mute all" }).click();
  await expect(page.getByRole("button", { name: "Unmute room" })).toBeVisible();
  const muteDirective = await page.evaluate(
    () => window.__UI2_SENT[window.__UI2_SENT.length - 1].args.body,
  );
  expect(String(muteDirective)).toContain("muted the room");
});

test("§7 perf: 5,000-message board — initial render, typing long-tasks, scroll fps", async ({ page }) => {
  await openUi2(page, 5000);

  // initial render: ms from navigation start until real feed content painted
  await expect(page.locator(".ui2-row").first()).toBeVisible();
  const renderMs = await page.evaluate(() => performance.now());

  // typing: count long tasks (>50ms) while typing 60 chars into the composer —
  // a paint stall from timeline re-render would register here
  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__LONGTASKS = 0;
    new PerformanceObserver((list) => {
      (window as unknown as Record<string, number>).__LONGTASKS += list.getEntries().length;
    }).observe({ entryTypes: ["longtask"] });
  });
  const composer = page.getByLabel("Compose message");
  await composer.click();
  await composer.pressSequentially("measuring keystroke to paint latency on a large board now", { delay: 0 });
  const longTasks = await page.evaluate(() => (window as unknown as Record<string, number>).__LONGTASKS);

  // scroll fps: drive the virtualized scroller for ~1s, count rAF frames
  const fps = await page.evaluate(
    () =>
      new Promise<number>((resolve) => {
        const scroller = document.querySelector("[data-virtuoso-scroller]") as HTMLElement;
        const start = performance.now();
        let frames = 0;
        const tick = () => {
          scroller.scrollTop += 120;
          frames++;
          if (performance.now() - start < 1000) requestAnimationFrame(tick);
          else resolve(frames / ((performance.now() - start) / 1000));
        };
        requestAnimationFrame(tick);
      }),
  );

  // eslint-disable-next-line no-console
  console.info(
    `[ui2-perf] 5k board: initial render=${renderMs.toFixed(0)}ms · long tasks while typing=${longTasks} · scroll=${fps.toFixed(1)}fps`,
  );

  expect(renderMs).toBeLessThan(1000); // §7: initial render < 1s
  expect(longTasks).toBeLessThanOrEqual(1); // §7 proxy: keystroke→paint < 16ms
  expect(fps).toBeGreaterThan(50); // §7: 60fps target with headless variance
});

test("mute is experience-first: feed stays silent even when traffic lands", async ({ page }) => {
  await openUi2(page, 200);
  await expect(page.locator(".ui2-row").first()).toBeVisible();
  await page.getByRole("button", { name: "Mute all" }).click();

  // simulate an agent posting anyway: append directly to the mock board
  await page.evaluate(() => {
    window.__TAURI_INTERNALS__.invoke("send_team_message", {
      dir: "x",
      to: "all",
      subject: "noise during mute",
      body: "should not surface",
      msg_type: "status",
      metadata: { __mock_from: "developer:0" },
    });
  });
  // force a real re-derive through the production path: the human's own send
  // triggers refresh() — the muted overlay must still keep the noise out
  const composer = page.getByLabel("Compose message");
  await composer.fill("human still speaks while muted");
  await composer.press("Enter");
  await expect(page.getByText("human still speaks while muted")).toBeVisible();
  await expect(page.getByText("should not surface")).toHaveCount(0); // silence held

  // unmute: the accrued traffic surfaces as ONE catch-up row, expandable —
  // never as ordinary rows (IA table §2, msg 364 coverage extension)
  await page.getByRole("button", { name: "Unmute room" }).click();
  const catchup = page.getByRole("button", { name: /caught up: \d+ events while muted/ });
  await expect(catchup).toBeVisible();
  await expect(page.getByText("should not surface")).toHaveCount(0); // still folded
  await catchup.click();
  await expect(page.getByText("noise during mute")).toBeVisible(); // audit on expand
});

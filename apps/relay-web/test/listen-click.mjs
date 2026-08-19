#!/usr/bin/env node
// Real-browser smoke: open a live listen URL, wait for P2P, click Listen.
import { createRequire } from "node:module";
import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";

const require = createRequire(import.meta.url);
const playwrightPath = [
  "/home/derpcat/.npm/_npx/e41f203b7505f1fb/node_modules/playwright/index.js",
  "/home/derpcat/.npm/_npx/e41f203b7505f1fb/node_modules/playwright/index.mjs",
].find((p) => existsSync(p));
if (!playwrightPath) {
  throw new Error("playwright not found in npx cache");
}
const { chromium, firefox } = require(playwrightPath);

const url = process.argv[2] || "https://relay.matari-audio.com/room-ce368463";
const outDir = new URL("../test-output/", import.meta.url);
await mkdir(outDir, { recursive: true });

async function sample(page) {
  return page.evaluate(() => ({
    revision: document.querySelector('meta[name="relay-listen"]')?.content ?? null,
    gateShow: document.getElementById("gate")?.classList.contains("show") ?? null,
    who: document.getElementById("who")?.textContent ?? null,
    cover: document.getElementById("coverL")?.style.height ?? "",
    audioHasStream: Boolean(document.getElementById("spkr")?.srcObject),
    logText: document.getElementById("log")?.textContent ?? "",
    whoFailed: (document.getElementById("who")?.textContent ?? "").includes("Connection failed"),
    relay: window.relay ?? null,
    csp: document.querySelector('meta[http-equiv="content-security-policy"]') ? true : null,
  }));
}

async function run(browserType, name) {
  const browser = await browserType.launch({ headless: true });
  const context = await browser.newContext();
  const page = await context.newPage();
  const consoleLines = [];
  const pageErrors = [];
  const ws = [];
  page.on("console", (msg) => consoleLines.push(`${msg.type()}: ${msg.text()}`));
  page.on("pageerror", (err) => pageErrors.push(String(err)));
  page.on("websocket", (socket) => {
    ws.push(`open ${socket.url()}`);
    socket.on("framereceived", (frame) => {
      const payload = String(frame.payload).slice(0, 160);
      ws.push(`recv ${payload}`);
    });
    socket.on("framesent", (frame) => {
      const payload = String(frame.payload).slice(0, 160);
      ws.push(`send ${payload}`);
    });
    socket.on("close", () => ws.push("close"));
  });

  const started = Date.now();
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 20000 });

  const covers = [];
  for (let i = 0; i < 8; i++) {
    await page.waitForTimeout(500);
    const snap = await sample(page);
    covers.push(snap.cover);
  }
  const beforeClick = await sample(page);

  const go = page.locator("#go");
  const visible = await go.isVisible().catch(() => false);
  let clickMs = null;
  let clickErr = null;
  try {
    const t = Date.now();
    await go.click({ timeout: 5000, force: true });
    clickMs = Date.now() - t;
  } catch (error) {
    clickErr = String(error);
  }

  let gateHiddenMs = null;
  let gateErr = null;
  try {
    const t = Date.now();
    await page.waitForFunction(
      () => !document.getElementById("gate")?.classList.contains("show"),
      { timeout: 2000 },
    );
    gateHiddenMs = Date.now() - t;
  } catch (error) {
    gateErr = String(error);
  }

  await page.waitForTimeout(9000);
  const after = await sample(page);
  const shot = new URL(`listen-${name}.png`, outDir);
  await page.screenshot({ path: shot.pathname, fullPage: true });
  await browser.close();

  const offerRecv = ws.some((line) => line.includes('"t":"offer"') || line.includes('"t": "offer"'));
  const answerSent = ws.some((line) => line.includes('"t":"answer"') || line.includes('"t": "answer"'));
  const beacon = consoleLines.filter((line) => line.includes("cloudflareinsights") || line.includes("beacon.min.js"));
  const meterMoved = covers.some((w) => w && w !== "100%" && w !== "");

  return {
    name,
    ms: Date.now() - started,
    visible,
    clickMs,
    clickErr,
    gateHiddenMs,
    gateErr,
    beforeClick,
    after,
    covers,
    meterMoved,
    offerRecv,
    answerSent,
    pageErrors,
    beacon,
    consoleLines: consoleLines.slice(0, 30),
    ws: ws.slice(0, 40),
    shot: shot.pathname,
  };
}

const results = [];
for (const [type, name] of [
  [chromium, "chromium"],
  [firefox, "firefox"],
]) {
  try {
    results.push(await run(type, name));
  } catch (error) {
    results.push({ name, fatal: String(error) });
  }
}

console.log(JSON.stringify(results, null, 2));
const blocked = results.filter((r) => {
  if (r.fatal || r.clickErr || r.gateErr) return true;
  if (r.after?.gateShow === true) return true;
  if (r.after?.whoFailed) return true;
  if (r.after?.revision && Number(r.after.revision) < 5) return true;
  return false;
});
process.exit(blocked.length ? 1 : 0);

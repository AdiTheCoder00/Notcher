// Check the built React UI with synthetic Rust events and a disposable browser profile.
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, extname, join, resolve } from "node:path";
import ts from "typescript";

const dist = resolve("dist");
const theme = ts.transpile(await readFile("src/lib/theme.ts", "utf8"), { module: ts.ModuleKind.ESNext });
const { defaultLoadout } = await import(`data:text/javascript;base64,${Buffer.from(theme).toString("base64")}`);
const shell = await readFile(join(dist, "index.html"), "utf8");
const setup = function (loadout) {
  const callbacks = new Map(), listeners = new Map();
  let next = 0, clipboardReads = 0, focusedOpens = 0, peekCloses = 0, peekRequests = 0, panelResizes = 0;
  let nativeFullOpen = false;
  const emit = (event, payload) => {
    for (const [id, entry] of listeners) if (entry.event === event) callbacks.get(entry.handler)?.({ event, id, payload });
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
  window.__TAURI_INTERNALS__ = {
    transformCallback(callback) { const id = ++next; callbacks.set(id, callback); return id; },
    async invoke(command, args = {}) {
      if (command === "plugin:event|listen") { const id = ++next; listeners.set(id, args); return id; }
      if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
      if (command === "get_loadout") return loadout;
      if (command === "list_tasks" || command === "list_installed_apps") return [];
      if (command === "dashboard_clipboard") return { kind: "text", data: `Synthetic copy ${++clipboardReads}` };
      if (command === "control_snapshot") return { media: null, volume: 0.5, muted: false, volume_supported: true, wifi: { supported: false, enabled: false }, bluetooth: { supported: false, enabled: false }, dark_mode: true };
      if (command === "close_control_centre") { nativeFullOpen = false; emit("control://state", { open: false }); }
      if (command === "open_control_centre") { nativeFullOpen = true; emit("control://state", { open: true, mode: "control" }); }
      if (command === "open_launcher") { nativeFullOpen = true; focusedOpens++; emit("control://state", { open: true, mode: "control" }); emit("control://focus", null); }
      if (command === "open_notch_peek") { peekRequests++; if (!nativeFullOpen) emit("control://state", { open: true, mode: "peek" }); }
      if (command === "dashboard_resize_panel") panelResizes++;
      if (command === "close_notch_peek") { peekCloses++; emit("control://state", { open: false }); }
    },
  };
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (condition) => { for (let n = 0; n < 200; n++) { if (condition()) return; await sleep(20); } throw new Error(`UI did not reach the expected state (focused opens: ${focusedOpens}; focus: ${document.activeElement?.outerHTML.slice(0, 160)})`); };
  const tab = (label) => [...document.querySelectorAll(".cc-overlay .cc-tab")].find((x) => x.textContent === label);
  const history = () => [...document.querySelectorAll(".cc-extra-footer button")].find((x) => /^(Start|Pause) history$/.test(x.textContent));
  window.addEventListener("DOMContentLoaded", async () => {
    const results = [];
    try {
      await wait(() => [...listeners.values()].some((x) => x.event === "control://state") && tab("Today"));
      const trigger = document.querySelector("button.notch-idle-shell");
      check(trigger?.getAttribute("aria-label") === "Open Control Centre", "Resting notch is not a named button");
      check(Math.abs(trigger.firstElementChild.getBoundingClientRect().top - trigger.getBoundingClientRect().top) < 0.5, "Button styling moved the resting pill away from the screen edge");
      window.notchKeyboardReady = true;
      await wait(() => focusedOpens === 1);
      await wait(() => document.activeElement === tab("Controls"));
      check(focusedOpens === 1, "Keyboard activation did not request native window focus");
      tab("Controls").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await wait(() => !document.querySelector(".notch-control-layer.is-open"));
      document.querySelector(".notch-idle-shell").dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: document.body }));
      await sleep(30);
      check(peekRequests === 0 && !document.querySelector(".cc-peek"), "Hover interrupted the full panel's native close");
      await wait(() => document.activeElement?.matches("button.notch-idle-shell"));
      check(!nativeFullOpen, "The frontend closed before the native full panel released its size");
      results.push("named notch button opens with focus and Escape restores it");
      results.push("hover during close cannot leave a preview in the full-size window");
      const resizesBeforePeek = panelResizes;
      emit("control://state", { open: true, mode: "peek" });
      await wait(() => document.querySelector(".cc-peek"));
      const layer = document.querySelector(".notch-control-layer");
      document.querySelector(".cc-peek").dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: layer }));
      await sleep(150);
      check(panelResizes === resizesBeforePeek, "Hover preview requested a full-panel resize");
      check(document.querySelector(".cc-peek") && peekCloses === 0, "Moving off the animating card closed the hover preview");
      layer.parentElement.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
      await wait(() => !document.querySelector(".cc-peek"));
      check(peekCloses === 1, "Leaving the notch window did not close the hover preview");
      results.push("hover stays open across the card boundary and closes outside the window");
      emit("control://state", { open: true, mode: "control" });
      tab("Today").click();
      await wait(() => document.querySelector(".cc-timer-presets button"));
      document.querySelector(".cc-timer-presets button").click();
      await sleep(40);
      document.querySelector(".cc-timer-actions .cc-action-primary").click();
      await wait(() => document.querySelector(".cc-timer-actions .cc-action-primary")?.textContent === "Pause");
      tab("Clipboard").click();
      await wait(() => history()?.textContent === "Start history");
      history().click();
      await wait(() => history()?.textContent === "Pause history");
      const panel = document.querySelector(".cc-persistent-panel"), reads = clipboardReads;
      emit("control://state", { open: false });
      await sleep(2300);
      check(document.querySelector(".cc-persistent-panel") === panel, "Closing destroyed the clipboard panel");
      check(clipboardReads > reads, "Clipboard polling stopped when the panel closed");
      results.push("clipboard continues while closed");
      emit("control://state", { open: true, mode: "peek" });
      await wait(() => document.querySelector(".cc-peek"));
      check(document.querySelector(".cc-persistent-panel") === panel, "Hover preview destroyed the full panel");
      emit("control://state", { open: false });
      emit("control://state", { open: true, mode: "control" });
      await wait(() => !document.querySelector(".cc-peek"));
      tab("Today").click();
      await wait(() => document.querySelector(".cc-timer-value"));
      check(document.querySelector(".cc-timer-actions .cc-action-primary").textContent === "Pause", "Closing reset the running timer");
      check(document.querySelector(".cc-timer-value").textContent.startsWith("0:"), "Timer did not count down while closed");
      results.push("timer survives close and hover preview");
      const seconds = () => document.querySelector(".cc-timer-value").textContent.split(":").reduce((a, x) => a * 60 + Number(x), 0);
      const before = seconds();
      const stalledAt = performance.now();
      while (performance.now() - stalledAt < 3200) {}
      await sleep(1100);
      check(before - seconds() >= 4, "Timer lost elapsed time during a delayed callback");
      const realNow = Date.now;
      Date.now = () => realNow() + 120_000;
      try {
        await wait(() => document.querySelector(".cc-timer-value").textContent === "Done");
      } finally { Date.now = realNow; }
      results.push("timer catches up after a UI stall and an elapsed deadline");
      tab("Clipboard").click();
      await wait(() => history()?.textContent === "Pause history");
      history().click();
      await wait(() => history()?.textContent === "Start history");
      results.push("clipboard can be paused after reopening");
      window.notchStateResult = { ok: true, results };
    } catch (error) { window.notchStateResult = { ok: false, error: String(error), results }; }
    const report = document.createElement("pre"); report.id = "notch-state-result"; report.textContent = JSON.stringify(window.notchStateResult); document.body.append(report);
  });
};
const html = shell.replace("<head>", `<head><script>(${setup})(${JSON.stringify(defaultLoadout())})</script>`);
const mime = { ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".jpg": "image/jpeg" };
const server = createServer(async (request, response) => {
  try {
    const pathname = new URL(request.url, "http://localhost").pathname;
    if (pathname === "/") { response.setHeader("content-type", "text/html"); response.end(html); return; }
    const file = resolve(dist, `.${decodeURIComponent(pathname)}`);
    if (!file.startsWith(dist + "/") && !file.startsWith(dist + "\\")) { response.writeHead(404).end(); return; }
    response.setHeader("content-type", mime[extname(file)] || "application/octet-stream");
    response.end(await readFile(file));
  } catch { response.writeHead(404).end(); }
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const work = await mkdtemp(join(tmpdir(), "murmur-state-"));
let browser, connection;
try {
  const edge = ["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Microsoft/Edge/Application/msedge.exe"].find(existsSync);
  if (!edge) throw new Error("Microsoft Edge is required for this UI check");
  // Real time is needed for focus restoration after Framer Motion's exit animation.
  browser = spawn(edge, ["--headless=new", "--disable-gpu", "--no-first-run", `--user-data-dir=${work}`, "--remote-debugging-address=127.0.0.1", "--remote-debugging-port=0", "about:blank"], { windowsHide: true, stdio: "ignore" });
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  let port;
  for (let n = 0; n < 100 && !port; n++) {
    try { port = (await readFile(join(work, "DevToolsActivePort"), "utf8")).split("\n")[0]; }
    catch { await sleep(50); }
  }
  if (!port) throw new Error("Browser did not expose its local DevTools port");
  const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  connection = new WebSocket(pages.find((page) => page.type === "page").webSocketDebuggerUrl);
  await new Promise((done, fail) => { connection.addEventListener("open", done, { once: true }); connection.addEventListener("error", fail, { once: true }); });
  let next = 0;
  const pending = new Map();
  connection.addEventListener("message", (event) => {
    const reply = JSON.parse(event.data), request = pending.get(reply.id);
    if (!request) return;
    pending.delete(reply.id);
    reply.error ? request.fail(new Error(JSON.stringify(reply.error))) : request.done(reply.result);
  });
  const send = (method, params = {}) => new Promise((done, fail) => {
    pending.set(++next, { done, fail });
    connection.send(JSON.stringify({ id: next, method, params }));
  });
  await send("Page.navigate", { url: `http://127.0.0.1:${server.address().port}/?window=notch` });
  let keyboardReady = false;
  for (let n = 0; n < 50 && !keyboardReady; n++) {
    await sleep(100);
    const reply = await send("Runtime.evaluate", { expression: "window.notchKeyboardReady", returnByValue: true });
    keyboardReady = reply.result?.value === true;
  }
  if (!keyboardReady) throw new Error("Resting notch did not become keyboard-ready");
  await send("Emulation.setFocusEmulationEnabled", { enabled: true });
  for (const [key, windowsVirtualKeyCode] of [["Tab", 9], ["Enter", 13]]) {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key, code: key, windowsVirtualKeyCode, ...(key === "Enter" ? { text: "\r", unmodifiedText: "\r" } : {}) });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key, code: key, windowsVirtualKeyCode });
  }
  let result;
  for (let n = 0; n < 150 && !result; n++) {
    await sleep(100);
    const reply = await send("Runtime.evaluate", { expression: "window.notchStateResult", returnByValue: true });
    result = reply.result?.value;
  }
  if (!result) throw new Error("Browser did not produce a UI test result");
  if (!result.ok) throw new Error(JSON.stringify(result));
  for (const check of result.results) console.log(`ok: ${check}`);
} finally {
  connection?.close();
  if (browser) {
    const exited = new Promise((done) => browser.once("exit", done));
    browser.kill();
    if (browser.exitCode === null) await exited;
  }
  server.close();
  if (dirname(work) !== resolve(tmpdir()) || !basename(work).startsWith("murmur-state-")) {
    throw new Error(`Refusing to remove an unexpected browser profile: ${work}`);
  }
  await rm(work, { recursive: true, force: true });
}

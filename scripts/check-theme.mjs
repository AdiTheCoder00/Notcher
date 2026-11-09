import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import ts from "typescript";

const properties = new Map();
let listener;
const media = {
  matches: false,
  addEventListener(_, fn) { listener = fn; },
  removeEventListener(_, fn) { if (listener === fn) listener = undefined; },
};
globalThis.document = { documentElement: { dataset: {}, style: { setProperty: (key, value) => properties.set(key, value) } } };
globalThis.window = { matchMedia: () => media };
const source = ts.transpile(await readFile("src/lib/theme.ts", "utf8"), { module: ts.ModuleKind.ESNext });
const { applyTheme } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const luminance = (hex) => [0.2126, 0.7152, 0.0722].reduce((sum, weight, index) => {
  const channel = parseInt(hex.slice(index * 2 + 1, index * 2 + 3), 16) / 255;
  return sum + weight * (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
}, 0);
for (const accent of ["#7c8cff", "#22d3ee", "#a855f7", "#f97316", "#10b981", "#f43f5e", "#000000", "#ffffff", "#0000ff", "#757575", "#767676"]) {
  applyTheme({ mode: "light", accent });
  const values = [luminance(accent), luminance(properties.get("--color-on-accent"))].sort((a, b) => a - b);
  assert.ok((values[1] + 0.05) / (values[0] + 0.05) >= 4.5, `Unreadable foreground on ${accent}`);
}
applyTheme({ mode: "system", accent: "#7c8cff" });
assert.equal(document.documentElement.dataset.theme, "dark");
media.matches = true;
listener();
assert.equal(document.documentElement.dataset.theme, "light");
applyTheme({ mode: "dark", accent: "#7c8cff" });
assert.equal(listener, undefined);
assert.equal(document.documentElement.dataset.theme, "dark");
console.log("ok: accent foreground contrast and live system-theme cleanup");

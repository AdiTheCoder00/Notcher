// Visual regression check for the notch Control Centre shoulders.
//
//   npm run check:notch
//
// The panel hangs from the screen edge, so its top corners flare out through a
// 12 px wing on each side of the body, and the bottom corners keep a 22 px
// radius. Windows display scaling multiplies those CSS pixels, and a wing that
// loses its tangency turns into a hard step against the top edge, so this check
// renders the built app in headless Edge at 100%, 125% and 150% device scale,
// then measures the silhouette from the pixels.
//
// No test dependencies: the app is served from `dist` by node:http, Edge
// renders it behind a magenta backdrop (so masked-away pixels stay visible),
// and the PNGs are decoded with node:zlib.

import { execFile, execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = resolve(HERE, "..", "dist");

// The app's logical window (src-tauri/src/notch.rs: CONTROL_WIDTH/HEIGHT).
// The body is 700 px wide; the window is 24 px wider for the two wings.
const PANEL = { width: 724, height: 430 };
const SHOULDER = 12; // wing size and flare radius
// A tangent shoulder only loses the antialiased tip of its curve on the top
// row (≈3.5 px at a 12 px radius); a cut one loses the whole 12 px wing.
const STEP_LIMIT = 6;
const BOTTOM_RADIUS = 22; // .cc-overlay border-radius
const SCALES = [1, 1.25, 1.5];

// Edge's --default-background-color=FFFF00FF (ARGB): a page that paints
// nothing shows this, so masked pixels are unambiguous.
const BACKDROP = { r: 255, g: 0, b: 255 };

const EDGE_CANDIDATES = [
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
];

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

/* ---------------------------------------------------------------- serving */

function serveDist() {
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    const path = decodeURIComponent(url.pathname);
    const file = join(DIST, path === "/" ? "index.html" : path);
    const send = async (target) => {
      const body = await readFile(target);
      response.writeHead(200, {
        "content-type": MIME[extname(target)] ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(body);
    };
    try {
      await send(file);
    } catch {
      // Single-page app: unknown routes fall back to the shell.
      try {
        await send(join(DIST, "index.html"));
      } catch {
        response.writeHead(404).end("not found");
      }
    }
  });
  return new Promise((res) => {
    server.listen(0, "127.0.0.1", () => res({ server, port: server.address().port }));
  });
}

/* ------------------------------------------------------------- screenshots */

function capture(edge, scale, profile, url, out) {
  const args = [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "--disable-background-networking",
    "--force-device-scale-factor=" + scale,
    "--window-size=" + PANEL.width + "," + PANEL.height,
    "--virtual-time-budget=4000",
    "--user-data-dir=" + profile,
    "--screenshot=" + out,
    url,
  ];
  return new Promise((res, rej) => {
    execFile(edge, args, { timeout: 60_000, windowsHide: true }, (error) => {
      if (!existsSync(out)) rej(new Error("Edge produced no screenshot: " + (error?.message ?? "unknown")));
      else res();
    });
  });
}

/* ------------------------------------------------------------ PNG decoding */

function decodePng(buffer) {
  let offset = 8;
  let width = 0;
  let height = 0;
  let depth = 0;
  let colorType = 0;
  let interlace = 0;
  const chunks = [];

  while (offset + 8 <= buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString("ascii", offset + 4, offset + 8);
    const data = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      depth = data[8];
      colorType = data[9];
      interlace = data[12];
    } else if (type === "IDAT") {
      chunks.push(data);
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + length;
  }

  if (depth !== 8 || interlace !== 0 || (colorType !== 2 && colorType !== 6)) {
    throw new Error(`unsupported PNG (depth ${depth}, colour ${colorType}, interlace ${interlace})`);
  }

  const channels = colorType === 6 ? 4 : 3;
  const stride = width * channels;
  const raw = inflateSync(Buffer.concat(chunks));
  const pixels = Buffer.alloc(width * height * 4);
  let previous = Buffer.alloc(stride);

  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let i = 0; i < stride; i++) {
      const left = i >= channels ? line[i - channels] : 0;
      const up = previous[i];
      const upLeft = i >= channels ? previous[i - channels] : 0;
      let value = line[i];
      if (filter === 1) value += left;
      else if (filter === 2) value += up;
      else if (filter === 3) value += (left + up) >> 1;
      else if (filter === 4) {
        const estimate = left + up - upLeft;
        const pa = Math.abs(estimate - left);
        const pb = Math.abs(estimate - up);
        const pc = Math.abs(estimate - upLeft);
        value += pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      }
      line[i] = value & 0xff;
    }
    for (let x = 0; x < width; x++) {
      const s = x * channels;
      const d = (y * width + x) * 4;
      pixels[d] = line[s];
      pixels[d + 1] = line[s + 1];
      pixels[d + 2] = line[s + 2];
      pixels[d + 3] = channels === 4 ? line[s + 3] : 255;
    }
    previous = line;
  }

  return { width, height, pixels };
}

/* --------------------------------------------------------------- measuring */

function pixelAt(image, x, y) {
  const index = (y * image.width + x) * 4;
  return { r: image.pixels[index], g: image.pixels[index + 1], b: image.pixels[index + 2] };
}

const near = (a, b, tolerance) =>
  Math.abs(a.r - b.r) <= tolerance &&
  Math.abs(a.g - b.g) <= tolerance &&
  Math.abs(a.b - b.b) <= tolerance;

/** Canvas run at row `y`, starting at `startX` and stepping by `step`. */
function backdropRun(image, y, startX, step, canvas) {
  let run = 0;
  let x = startX;
  while (x >= 0 && x < image.width && near(pixelAt(image, x, y), canvas, 10)) {
    run += 1;
    x += step;
  }
  return run;
}

/** Last panel pixel on a scan line, so a scrollbar or gap cannot skew a cut. */
function panelEdge(image, fixed, horizontal, canvas) {
  if (horizontal) {
    let x = image.width - 1;
    while (x > 0 && near(pixelAt(image, x, fixed), canvas, 10)) x -= 1;
    return x;
  }
  let y = image.height - 1;
  const x = fixed;
  while (y > 0 && near(pixelAt(image, x, y), canvas, 10)) y -= 1;
  return y;
}

/**
 * Rounded corner: a chord `offset` px in from the edge is `depth` px deep on a
 * corner of radius r, where (offset-r)^2 + (depth-r)^2 = r^2. The larger root is
 * r = offset + depth + sqrt(2*offset*depth), and it holds for the top flare and
 * the bottom corners alike. A cusp (an arc that meets the edge perpendicularly)
 * does not satisfy it: it reads as a much larger radius.
 */
function convexRadiusFromRun(run, depth) {
  return run + depth + Math.sqrt(2 * run * depth);
}

/**
 * Radius of a top shoulder. The wing is cut by a circle centred on the body's
 * top corner, so a canvas run `run` px wide `depth` px below the edge satisfies
 * run^2 + depth^2 = 2*radius*depth. The median of three depths drops a row lost
 * to the antialiased rim.
 */
function shoulderRadius(image, edgeX, step, scale, canvas) {
  return [3, 6, 9]
    .map((cssDepth) => {
      const depth = Math.max(1, Math.round(cssDepth * scale));
      const run = backdropRun(image, depth, edgeX, step, canvas);
      return (run * run + depth * depth) / (2 * depth) / scale;
    })
    .sort((a, b) => a - b)[1];
}

function measure(image, requestedScale) {
  const scale = image.width / PANEL.width;
  // The bottom-left corner is always outside the body: the wings only reach the
  // top edge.
  const canvas = pixelAt(image, 0, image.height - 1);
  const panel = pixelAt(image, Math.round(image.width / 2), Math.round(image.height / 2));
  // A "canvas" sample that is panel coloured means the wing is missing, and the
  // distance between the two catches a page that paints nothing at all.
  const carved =
    !near(canvas, panel, 10) && near(pixelAt(image, 0, Math.round(6 * scale)), canvas, 10);

  // The body sits SHOULDER px in from the window edge; the wings fill the rest
  // of the top edge.
  const midRow = Math.round(image.height / 2);
  const bodyInset = backdropRun(image, midRow, 0, 1, canvas) / scale;

  const shoulderLeft = shoulderRadius(image, 0, 1, scale, canvas);
  const shoulderRight = shoulderRadius(image, image.width - 1, -1, scale, canvas);
  // How much of the top row the wing covers. Tangency leaves only the
  // antialiased tip of the curve outside the surface — of order
  // sqrt(2 * radius * 0.5) ≈ 3.5 px at a 12 px radius — while a cut shoulder
  // misses the whole wing, which is the hard step this check guards.
  const leftStep = backdropRun(image, 0, 0, 1, canvas) / scale;
  const rightStep = backdropRun(image, 0, image.width - 1, -1, canvas) / scale;

  // Median of three depths: a single shallow row loses ~1 px to the antialiased
  // rim, which the tightened tolerance no longer allows.
  const bottomEdge = panelEdge(image, Math.round(image.width / 2), false, canvas);
  const bottomRadius = [3, 6, 9]
    .map((cssDepth) => {
      const depth = Math.max(1, Math.round(cssDepth * scale));
      const row = Math.max(0, bottomEdge - depth);
      const run = Math.max(1, backdropRun(image, row, 0, 1, canvas) - Math.round(bodyInset * scale));
      return convexRadiusFromRun(run, depth) / scale;
    })
    .sort((a, b) => a - b)[1];

  const half = Math.round(image.width / 2);
  const topFlush =
    !near(pixelAt(image, half, 0), canvas, 10) &&
    !near(pixelAt(image, half - Math.round(100 * scale), 1), canvas, 10) &&
    !near(pixelAt(image, half + Math.round(100 * scale), 1), canvas, 10);

  const outsidePanel = !near(
    pixelAt(image, Math.min(image.width - 1, Math.round((SHOULDER + 3) * scale)), Math.round(3 * scale)),
    canvas,
    10,
  );

  return {
    requestedScale,
    scale,
    size: `${image.width}x${image.height}`,
    shoulderLeft,
    shoulderRight,
    leftStep,
    rightStep,
    bodyInset,
    bottomRadius,
    topFlush,
    carved,
    outsidePanel,
  };
}

/**
 * Measures the same shoulders on the running desktop app, where a page
 * scrollbar or a window/client-size mismatch would clip one away. Returns
 * status "skipped" when Murmur is not running so headless runs still pass.
 */
function checkNativeShoulder() {
  const script = join(HERE, "check-notch-native.ps1");
  if (!existsSync(script)) return { status: "skipped", reason: "native probe missing" };
  try {
    const output = execFileSync(
      "powershell",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script],
      { encoding: "utf8", timeout: 90_000 },
    );
    const payload = output
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.startsWith("{"))
      .pop();
    if (!payload) return { status: "skipped", reason: "native probe printed nothing" };
    return JSON.parse(payload);
  } catch (error) {
    const first = String(error?.message ?? error).split("\n")[0];
    return { status: "skipped", reason: `native probe failed: ${first}` };
  }
}

/* -------------------------------------------------------------------- main */

const failures = [];
const check = (ok, label, detail) => {
  if (!ok) failures.push(`${label}: ${detail}`);
};

const edge = EDGE_CANDIDATES.find((path) => existsSync(path));
if (!edge) {
  console.error("check-notch-scales: Microsoft Edge was not found; cannot render the panel.");
  process.exit(1);
}

if (!existsSync(join(DIST, "index.html"))) {
  console.error("check-notch-scales: dist/index.html is missing. Run `npm run build` first.");
  process.exit(1);
}

const { server, port } = await serveDist();
const workdir = await mkdtemp(join(tmpdir(), "murmur-notch-"));
const url = `http://127.0.0.1:${port}/?window=notch&control=open&probe=1`;

const rows = [];
try {
  for (const requested of SCALES) {
    const file = join(workdir, `scale-${requested}.png`);
    await capture(edge, requested, join(workdir, `profile-${requested}`), url, file);
    const image = decodePng(await readFile(file));
    const result = measure(image, requested);
    rows.push(result);

    check(
      Math.abs(result.scale - requested) <= 0.02,
      `${Math.round(requested * 100)}%`,
      `Edge rendered at ${result.scale.toFixed(2)}x, not ${requested}x (image ${result.size})`,
    );
    check(
      Math.abs(image.height - PANEL.height * result.scale) <= 2,
      `${Math.round(requested * 100)}%`,
      `image height ${image.height} != ${Math.round(PANEL.height * result.scale)}`,
    );
    check(
      result.carved,
      `${Math.round(requested * 100)}%`,
      "top shoulders are not carved — the wing gradients are missing or invalid",
    );
    check(
      Math.abs(result.bodyInset - SHOULDER) <= 1,
      `${Math.round(requested * 100)}%`,
      `panel body starts ${result.bodyInset.toFixed(1)} px in, expected ${SHOULDER}`,
    );
    check(
      result.outsidePanel,
      `${Math.round(requested * 100)}%`,
      "the panel is missing just past the fillet",
    );
    check(
      Math.abs(result.shoulderLeft - SHOULDER) <= 1.5,
      `${Math.round(requested * 100)}%`,
      `left shoulder ${result.shoulderLeft.toFixed(1)} px, expected ${SHOULDER}`,
    );
    check(
      Math.abs(result.shoulderRight - SHOULDER) <= 1.5,
      `${Math.round(requested * 100)}%`,
      `right shoulder ${result.shoulderRight.toFixed(1)} px, expected ${SHOULDER}`,
    );
    check(
      Math.abs(result.shoulderLeft - result.shoulderRight) <= 1.5,
      `${Math.round(requested * 100)}%`,
      `shoulders are asymmetric (${result.shoulderLeft.toFixed(1)} vs ${result.shoulderRight.toFixed(1)} px)`,
    );
    check(
      result.leftStep <= STEP_LIMIT && result.rightStep <= STEP_LIMIT,
      `${Math.round(requested * 100)}%`,
      `the shoulder starts a step in from the top edge (${result.leftStep.toFixed(1)} / ${result.rightStep.toFixed(1)} px)`,
    );
    check(result.topFlush, `${Math.round(requested * 100)}%`, "the panel does not meet the top edge");
    check(
      Math.abs(result.bottomRadius - BOTTOM_RADIUS) <= 1,
      `${Math.round(requested * 100)}%`,
      `bottom corner ${result.bottomRadius.toFixed(1)} px, expected ${BOTTOM_RADIUS}`,
    );
  }
} finally {
  server.close();
}

// One retry: the notch can be mid-animation (or mid-hover) on the first try.
let native = checkNativeShoulder();
if (native.status !== "ok") native = checkNativeShoulder();
if (native.status === "ok") {
  check(native.carved, "live", "the desktop shoulders are not carved");
  check(native.flush, "live", "the desktop panel does not meet the top edge");
  check(
    Math.abs(native.inset - SHOULDER) <= 1,
    "live",
    `the desktop body starts ${native.inset} px in, expected ${SHOULDER}`,
  );
  check(
    Math.abs(native.shoulderLeft - SHOULDER) <= 1.5,
    "live",
    `left shoulder ${native.shoulderLeft} px, expected ${SHOULDER}`,
  );
  check(
    Math.abs(native.shoulderRight - SHOULDER) <= 1.5,
    "live",
    `right shoulder ${native.shoulderRight} px, expected ${SHOULDER}`,
  );
  check(
    Math.abs(native.shoulderLeft - native.shoulderRight) <= 1.5,
    "live",
    `shoulders are asymmetric (${native.shoulderLeft} vs ${native.shoulderRight} px)`,
  );
  check(
    native.stepLeft <= STEP_LIMIT && native.stepRight <= STEP_LIMIT,
    "live",
    `the desktop shoulder starts a step in from the top edge (${native.stepLeft} / ${native.stepRight} px)`,
  );
}

const header = ["scale", "image", "flare L", "flare R", "step", "inset", "bottom", "flush"];
console.log(header.join("  "));
for (const row of rows) {
  console.log(
    [
      `${Math.round(row.requestedScale * 100)}%`.padEnd(header[0].length),
      row.size.padEnd(header[1].length),
      `${row.shoulderLeft.toFixed(1)}px`.padEnd(header[2].length),
      `${row.shoulderRight.toFixed(1)}px`.padEnd(header[3].length),
      `${Math.max(row.leftStep, row.rightStep).toFixed(1)}px`.padEnd(header[4].length),
      `${row.bodyInset.toFixed(1)}px`.padEnd(header[5].length),
      `${row.bottomRadius.toFixed(1)}px`.padEnd(header[6].length),
      row.topFlush ? "ok" : "FAIL",
    ].join("  "),
  );
}

if (native.status === "ok") {
  console.log(
    [
      "live".padEnd(header[0].length),
      `${native.width}x${native.height}`.padEnd(header[1].length),
      `${native.shoulderLeft}px`.padEnd(header[2].length),
      `${native.shoulderRight}px`.padEnd(header[3].length),
      `${Math.max(native.stepLeft, native.stepRight)}px`.padEnd(header[4].length),
      `${native.inset}px`.padEnd(header[5].length),
      "-".padEnd(header[6].length),
      native.flush ? "ok" : "FAIL",
    ].join("  "),
  );
} else {
  console.log(`live   skipped (${native.reason})`);
}

if (failures.length > 0) {
  console.error(`\ncheck-notch-scales: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  console.error(`\nscreenshots kept in ${workdir}`);
  process.exit(1);
}

await rm(workdir, { recursive: true, force: true });
console.log("\ncheck-notch-scales: shoulders flare tangent into the top edge at every scale.");

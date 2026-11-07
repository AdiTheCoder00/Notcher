// Generates src-tauri/icons/* deterministically, with no external tooling.
//
//   node scripts/make-icons.mjs
//
// Writes 32x32.png, 128x128.png, 128x128@2x.png, icon.png (512) and a
// multi-resolution, BMP-encoded icon.ico (the format tauri-winres embeds).

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(HERE, "..", "src-tauri", "icons");

const clamp01 = (n) => (n < 0 ? 0 : n > 1 ? 1 : n);

/** Signed distance to a rounded rectangle centred on the icon. */
function sdRoundedRect(x, y, halfW, halfH, radius) {
  const dx = Math.abs(x) - (halfW - radius);
  const dy = Math.abs(y) - (halfH - radius);
  const ax = dx > 0 ? dx : 0;
  const ay = dy > 0 ? dy : 0;
  return Math.sqrt(ax * ax + ay * ay) + Math.min(Math.max(dx, dy), 0) - radius;
}

/** Renders one icon into a straight-alpha RGBA buffer. */
function render(size) {
  const px = Buffer.alloc(size * size * 4);
  const pad = size * 0.055;
  const half = size / 2 - pad;
  const radius = size * 0.235;

  // Three "sound wave" bars: [centre offset, half height] in units of size.
  const bars = [
    [-0.155, 0.105],
    [0.0, 0.185],
    [0.155, 0.135],
  ];
  const barHalfW = size * 0.045;

  const SS = 4; // supersampling per axis
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let bg = 0;
      let fg = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const cx = x + (sx + 0.5) / SS - size / 2;
          const cy = y + (sy + 0.5) / SS - size / 2;

          if (sdRoundedRect(cx, cy, half, half, radius) <= 0) bg += 1;
          for (const [ox, oy] of bars) {
            const bx = cx - ox * size;
            const by = cy - oy * size;
            const bh = oy_half(size, oy);
            if (sdRoundedRect(bx, by, barHalfW, bh, barHalfW) <= 0) {
              fg += 1;
              break;
            }
          }
        }
      }
      const total = SS * SS;
      const bgA = bg / total;
      const fgA = fg / total;

      // Diagonal indigo -> cyan gradient across the tile.
      const t = clamp01((x / size) * 0.55 + (y / size) * 0.45);
      const bgR = 99 + (34 - 99) * t;
      const bgG = 102 + (211 - 102) * t;
      const bgB = 241 + (238 - 241) * t;

      // Composite white bars over the gradient tile (source-over, straight alpha).
      const barA = fgA * 0.96;
      const outA = bgA + barA * (1 - bgA);
      const mix = outA === 0 ? 0 : barA / outA;
      const r = bgR * (1 - mix) + 255 * mix;
      const g = bgG * (1 - mix) + 255 * mix;
      const b = bgB * (1 - mix) + 255 * mix;

      const i = (y * size + x) * 4;
      px[i] = Math.round(r);
      px[i + 1] = Math.round(g);
      px[i + 2] = Math.round(b);
      px[i + 3] = Math.round(outA * 255);
    }
  }
  return px;
}

// bar half-height in pixels, derived from the "half height" spec value
function oy_half(size, halfHeight) {
  return halfHeight * size;
}

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function encodePng(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  const stride = size * 4;
  const raw = Buffer.alloc((stride + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (stride + 1)] = 0; // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** One ICO entry as an uncompressed 32bpp DIB (maximum Windows compatibility). */
function encodeDib(size, rgba) {
  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0); // biSize
  header.writeInt32LE(size, 4); // biWidth
  header.writeInt32LE(size * 2, 8); // biHeight (image + mask)
  header.writeUInt16LE(1, 12); // biPlanes
  header.writeUInt16LE(32, 14); // biBitCount
  header.writeUInt32LE(0, 16); // biCompression = BI_RGB
  header.writeUInt32LE(size * size * 4, 20); // biSizeImage

  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    const srcRow = size - 1 - y; // DIB rows are bottom-up
    for (let x = 0; x < size; x++) {
      const s = (srcRow * size + x) * 4;
      const d = (y * size + x) * 4;
      pixels[d] = rgba[s + 2]; // B
      pixels[d + 1] = rgba[s + 1]; // G
      pixels[d + 2] = rgba[s]; // R
      pixels[d + 3] = rgba[s + 3]; // A
    }
  }

  const maskRowBytes = Math.ceil(size / 32) * 4;
  const mask = Buffer.alloc(maskRowBytes * size); // all zero = fully opaque

  return Buffer.concat([header, pixels, mask]);
}

function encodeIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(entries.length, 4);

  const dir = Buffer.alloc(16 * entries.length);
  let offset = header.length + dir.length;
  entries.forEach(({ size, data }, i) => {
    const d = i * 16;
    dir[d] = size >= 256 ? 0 : size;
    dir[d + 1] = size >= 256 ? 0 : size;
    dir[d + 2] = 0;
    dir[d + 3] = 0;
    dir.writeUInt16LE(1, d + 4); // planes
    dir.writeUInt16LE(32, d + 6); // bit count
    dir.writeUInt32LE(data.length, d + 8);
    dir.writeUInt32LE(offset, d + 12);
    offset += data.length;
  });

  return Buffer.concat([header, dir, ...entries.map((e) => e.data)]);
}

mkdirSync(OUT_DIR, { recursive: true });

const pngs = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
];

for (const [name, size] of pngs) {
  writeFileSync(resolve(OUT_DIR, name), encodePng(size, render(size)));
  console.log(`wrote icons/${name} (${size}x${size})`);
}

const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const ico = encodeIco(
  icoSizes.map((size) => ({ size, data: encodeDib(size, render(size)) })),
);
writeFileSync(resolve(OUT_DIR, "icon.ico"), ico);
console.log(`wrote icons/icon.ico (${icoSizes.join(", ")})`);

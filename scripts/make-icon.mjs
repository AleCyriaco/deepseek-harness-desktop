// Generates the app icon (`app-icon.png`, 1024x1024) with no dependencies:
// a hand-rolled PNG encoder (zlib + CRC32) and a small rasteriser for the
// DeepSeek whale mark in `assets/deepseek-whale.svg`. `tauri icon` then
// produces every platform size:
//
//   node scripts/make-icon.mjs
//   npx tauri icon app-icon.png
import { deflateSync } from "node:zlib";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const S = 1024;
const root = join(dirname(fileURLToPath(import.meta.url)), "..");

// ---- PNG encoder ---------------------------------------------------------

const crcTable = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])));
  return Buffer.concat([len, typeBuf, data, crc]);
}

function encodePng(rgba, w, h) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4);
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([
    sig,
    chunk("IHDR", ihdr),
    chunk("IDAT", idat),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// ---- SVG path -> polygons ------------------------------------------------

// Supports the subset the whale mark uses: M/L/C/Z, absolute and relative,
// including implicit command repetition. Anything else throws rather than
// silently drawing the wrong shape.
const FLATTEN_STEPS = 24;

function parsePath(d) {
  const tokens = d.match(/[A-Za-z]|-?\d*\.?\d+(?:[eE][-+]?\d+)?/g) ?? [];
  const subpaths = [];
  let current = null;
  let cmd = null;
  let x = 0;
  let y = 0;
  let startX = 0;
  let startY = 0;
  let i = 0;

  const num = () => {
    const value = Number(tokens[i++]);
    if (Number.isNaN(value)) throw new Error(`bad number near token ${i} in path`);
    return value;
  };
  const close = () => {
    if (current && current.length > 1) subpaths.push(current);
    current = null;
  };

  while (i < tokens.length) {
    if (/[A-Za-z]/.test(tokens[i])) cmd = tokens[i++];
    if (cmd === undefined) throw new Error("path does not start with a command");
    const rel = cmd === cmd.toLowerCase();

    switch (cmd.toUpperCase()) {
      case "M": {
        close();
        const nx = num();
        const ny = num();
        x = rel ? x + nx : nx;
        y = rel ? y + ny : ny;
        startX = x;
        startY = y;
        current = [[x, y]];
        // Further coordinate pairs after an M are implicit L.
        cmd = rel ? "l" : "L";
        break;
      }
      case "L": {
        const nx = num();
        const ny = num();
        x = rel ? x + nx : nx;
        y = rel ? y + ny : ny;
        current.push([x, y]);
        break;
      }
      case "C": {
        const x0 = x;
        const y0 = y;
        const c1x = rel ? x + num() : num();
        const c1y = rel ? y + num() : num();
        const c2x = rel ? x + num() : num();
        const c2y = rel ? y + num() : num();
        const ex = rel ? x + num() : num();
        const ey = rel ? y + num() : num();
        for (let s = 1; s <= FLATTEN_STEPS; s++) {
          const t = s / FLATTEN_STEPS;
          const u = 1 - t;
          current.push([
            u * u * u * x0 + 3 * u * u * t * c1x + 3 * u * t * t * c2x + t * t * t * ex,
            u * u * u * y0 + 3 * u * u * t * c1y + 3 * u * t * t * c2y + t * t * t * ey,
          ]);
        }
        x = ex;
        y = ey;
        break;
      }
      case "Z": {
        close();
        x = startX;
        y = startY;
        break;
      }
      default:
        throw new Error(`unsupported path command "${cmd}"`);
    }
  }
  close();
  return subpaths;
}

/// Scanline fill with the nonzero winding rule, supersampled for antialiasing.
/// Returns per-pixel coverage in 0..1.
function rasterize(subpaths, w, h, ss = 4) {
  const coverage = new Float32Array(w * h);
  const edges = [];
  for (const points of subpaths) {
    for (let i = 0; i < points.length; i++) {
      const [x0, y0] = points[i];
      const [x1, y1] = points[(i + 1) % points.length];
      if (y0 !== y1) edges.push([x0, y0, x1, y1]);
    }
  }

  const share = 1 / (ss * ss);
  const crossings = [];
  for (let sy = 0; sy < h * ss; sy++) {
    const y = (sy + 0.5) / ss;
    crossings.length = 0;
    for (const [x0, y0, x1, y1] of edges) {
      if ((y >= y0 && y < y1) || (y >= y1 && y < y0)) {
        const t = (y - y0) / (y1 - y0);
        crossings.push([x0 + t * (x1 - x0), y1 > y0 ? 1 : -1]);
      }
    }
    if (crossings.length < 2) continue;
    crossings.sort((a, b) => a[0] - b[0]);

    const row = Math.floor(sy / ss) * w;
    let winding = 0;
    for (let k = 0; k < crossings.length - 1; k++) {
      winding += crossings[k][1];
      if (winding === 0) continue;
      const from = Math.max(0, Math.round(crossings[k][0] * ss));
      const to = Math.min(w * ss, Math.round(crossings[k + 1][0] * ss));
      for (let sx = from; sx < to; sx++) coverage[row + Math.floor(sx / ss)] += share;
    }
  }
  return coverage;
}

// ---- drawing -------------------------------------------------------------

const inRoundedRect = (x, y, x0, y0, x1, y1, r) => {
  const cx = Math.max(x0 + r, Math.min(x, x1 - r));
  const cy = Math.max(y0 + r, Math.min(y, y1 - r));
  const dx = x - cx;
  const dy = y - cy;
  return (
    dx * dx + dy * dy <= r * r ||
    (x >= x0 && x <= x1 && y >= cy - r && y <= cy + r) ||
    (y >= y0 && y <= y1 && x >= cx - r && x <= cx + r)
  );
};

const lerp = (a, b, t) => a + (b - a) * t;
const hex = (h) => [
  parseInt(h.slice(1, 3), 16),
  parseInt(h.slice(3, 5), 16),
  parseInt(h.slice(5, 7), 16),
];

const top = hex("#6f8bff");
const bottom = hex("#2743d0");

// The whale mark, scaled to this fraction of the canvas and centred.
const MARK_SCALE = 0.62;

const svg = readFileSync(join(root, "assets", "deepseek-whale.svg"), "utf8");
const d = svg.match(/\sd="([^"]+)"/)?.[1];
if (!d) throw new Error("no path data found in assets/deepseek-whale.svg");

const subpaths = parsePath(d);

// Fit the mark's own bounding box into the canvas, so the icon stays centred
// even if the upstream artwork changes its padding.
let minX = Infinity;
let minY = Infinity;
let maxX = -Infinity;
let maxY = -Infinity;
for (const points of subpaths) {
  for (const [px, py] of points) {
    if (px < minX) minX = px;
    if (py < minY) minY = py;
    if (px > maxX) maxX = px;
    if (py > maxY) maxY = py;
  }
}
const scale = (S * MARK_SCALE) / Math.max(maxX - minX, maxY - minY);
const offsetX = (S - (maxX - minX) * scale) / 2 - minX * scale;
const offsetY = (S - (maxY - minY) * scale) / 2 - minY * scale;
const placed = subpaths.map((points) =>
  points.map(([px, py]) => [px * scale + offsetX, py * scale + offsetY]),
);

const mark = rasterize(placed, S, S);
const px = Buffer.alloc(S * S * 4);

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    let r = 0;
    let g = 0;
    let b = 0;
    let a = 0;
    if (inRoundedRect(x, y, 24, 24, S - 24, S - 24, 200)) {
      const t = y / S;
      r = lerp(top[0], bottom[0], t);
      g = lerp(top[1], bottom[1], t);
      b = lerp(top[2], bottom[2], t);
      a = 255;

      const alpha = Math.min(1, mark[y * S + x]);
      if (alpha > 0) {
        r = lerp(r, 255, alpha);
        g = lerp(g, 255, alpha);
        b = lerp(b, 255, alpha);
      }
    }
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = a;
  }
}

writeFileSync(join(root, "app-icon.png"), encodePng(px, S, S));
console.log(`wrote app-icon.png (${S}x${S})`);

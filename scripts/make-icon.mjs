// Generates a clean, dependency-free app icon (`app-icon.png`, 1024x1024)
// with a hand-rolled PNG encoder (zlib + CRC32), then lets `tauri icon`
// produce every platform size.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 1024;

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

// ---- drawing -------------------------------------------------------------

const inRoundedRect = (x, y, x0, y0, x1, y1, r) => {
  const cx = Math.max(x0 + r, Math.min(x, x1 - r));
  const cy = Math.max(y0 + r, Math.min(y, y1 - r));
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r || (x >= x0 && x <= x1 && y >= cy - r && y <= cy + r) || (y >= y0 && y <= y1 && x >= cx - r && x <= cx + r);
};

const distToSeg = (px, py, ax, ay, bx, by) => {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  let t = len2 === 0 ? 0 : ((px - ax) * dx + (py - ay) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
};

const lerp = (a, b, t) => a + (b - a) * t;
const hex = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];

const top = hex("#6f8bff");
const bottom = hex("#2743d0");

const px = Buffer.alloc(S * S * 4);

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    let r = 0, g = 0, b = 0, a = 0;
    if (inRoundedRect(x, y, 24, 24, S - 24, S - 24, 200)) {
      const t = y / S;
      r = lerp(top[0], bottom[0], t);
      g = lerp(top[1], bottom[1], t);
      b = lerp(top[2], bottom[2], t);
      a = 255;

      // A ">_" terminal mark in white.
      const dChevron = Math.min(
        distToSeg(x, y, 300, 350, 470, 512),
        distToSeg(x, y, 470, 512, 300, 674),
      );
      const dUnderscore = distToSeg(x, y, 560, 640, 740, 640);
      const dMark = Math.min(dChevron, dUnderscore);
      const mark = dMark < 56 ? 1 : dMark < 70 ? 1 - (dMark - 56) / 14 : 0;
      if (mark > 0) {
        r = lerp(r, 255, mark);
        g = lerp(g, 255, mark);
        b = lerp(b, 255, mark);
      }
    }
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = a;
  }
}

writeFileSync("app-icon.png", encodePng(px, S, S));
console.log("wrote app-icon.png (1024x1024)");

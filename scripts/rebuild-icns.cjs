// Rebuild icon.icns more compactly. We don't have an iconset folder, so we
// (a) read the existing icns, (b) re-encode every PNG inside with oxipng at
//     max, (c) drop the redundant 1024x1024 (ic10) and 512@2x (ic14) entries.
// macOS looks for ic07/ic08/ic09/ic11/ic12/ic13 + legacy is32/il32/s8mk/l8mk
// for a Retina-aware app icon; the 1024 sizes are unused in the modern
// macOS UI and most dock surfaces.

const fs = require("node:fs");
const path = require("node:path");
const cp = require("node:child_process");

const oxipng =
  "node_modules/oxipng/bin/oxipng-4.0.3-x86_64-pc-windows-msvc/oxipng.exe";

function optPng(buf) {
  const tmp = path.join("src-tauri", "icons", ".tmp_opt.png");
  fs.writeFileSync(tmp, buf);
  cp.execFileSync(oxipng, ["--strip", "safe", "--alpha", "safe", "-o", "max", tmp], {
    stdio: "ignore",
  });
  const out = fs.readFileSync(tmp);
  fs.unlinkSync(tmp);
  return out;
}

const orig = fs.readFileSync("src-tauri/icons/icon.icns");
if (orig.readUInt32BE(0) !== 0x69636e73) {
  throw new Error("not an icns file");
}

const drop = new Set(["ic10", "ic14", "ic04", "ic05", "ic06"]);

let off = 8;
const out = [];
while (off + 8 < orig.length) {
  const sz = orig.readUInt32BE(off + 4);
  if (sz === 0) break;
  const t = orig.slice(off, off + 4).toString("ascii");
  const payload = orig.slice(off + 8, off + sz);

  if (drop.has(t)) {
    console.log("drop", t, sz, "bytes");
    off += sz;
    continue;
  }

  // Re-encode PNGs in-place; pass through legacy formats.
  const isPng =
    payload.length > 8 &&
    payload[0] === 0x89 &&
    payload[1] === 0x50 &&
    payload[2] === 0x4e &&
    payload[3] === 0x47;
  const newPayload = isPng ? optPng(payload) : payload;
  const newSize = newPayload.length + 8;
  const sizeBuf = Buffer.alloc(4);
  sizeBuf.writeUInt32BE(newSize, 0);
  out.push(Buffer.from(t, "ascii"), sizeBuf, newPayload);
  console.log("keep", t, sz, "->", newSize, "bytes");
  off += sz;
}

const body = Buffer.concat(out);
const header = Buffer.alloc(8);
"icns".split("").forEach((c, i) => (header[i] = c.charCodeAt(0)));
header.writeUInt32BE(body.length + 8, 4);
fs.writeFileSync("src-tauri/icons/icon.icns.new", Buffer.concat([header, body]));
console.log("\nwritten src-tauri/icons/icon.icns.new", body.length + 8, "bytes");

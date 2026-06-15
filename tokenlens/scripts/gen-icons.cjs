// Generate placeholder icons for Tauri build.
// Run: node scripts/gen-icons.cjs
const zlib = require('zlib');
const fs = require('fs');
const path = require('path');

const TARGET_DIR = path.resolve(__dirname, '..', 'src-tauri', 'icons');

function makePng(size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    for (let x = 0; x < size; x++) {
      const i = y * (size * 4 + 1) + 1 + x * 4;
      const dx = x - size / 2;
      const dy = y - size / 2;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const outer = size * 0.4;
      const inner = size * 0.18;
      if (dist < outer) {
        if (dist > inner) {
          raw[i] = 80; raw[i + 1] = 220; raw[i + 2] = 200; raw[i + 3] = 255;
        } else {
          raw[i] = 20; raw[i + 1] = 20; raw[i + 2] = 30; raw[i + 3] = 255;
        }
      } else {
        raw[i] = 20; raw[i + 1] = 20; raw[i + 2] = 30; raw[i + 3] = 255;
      }
    }
  }
  const compressed = zlib.deflateSync(raw);
  const crcTable = (() => {
    const t = [];
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = (c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
      t[n] = c >>> 0;
    }
    return t;
  })();
  const crc32 = (buf) => {
    let crc = 0xFFFFFFFF;
    for (const b of buf) crc = (crc >>> 8) ^ crcTable[(crc ^ b) & 0xFF];
    return (crc ^ 0xFFFFFFFF) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length, 0);
    const typeBuf = Buffer.from(type);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
    return Buffer.concat([len, typeBuf, data, crc]);
  };
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([sig, chunk('IHDR', ihdr), chunk('IDAT', compressed), chunk('IEND', Buffer.alloc(0))]);
}

fs.mkdirSync(TARGET_DIR, { recursive: true });
fs.writeFileSync(path.join(TARGET_DIR, '32x32.png'), makePng(32));
fs.writeFileSync(path.join(TARGET_DIR, '128x128.png'), makePng(128));
fs.writeFileSync(path.join(TARGET_DIR, '128x128@2x.png'), makePng(256));
fs.writeFileSync(path.join(TARGET_DIR, 'icon.png'), makePng(512));

// Minimal ICO embedding 32x32 PNG
const ico32 = makePng(32);
const icoHeader = Buffer.alloc(6);
icoHeader.writeUInt16LE(0, 0);
icoHeader.writeUInt16LE(1, 2);
icoHeader.writeUInt16LE(1, 4);
const icoDir = Buffer.alloc(16);
icoDir[0] = 32; icoDir[1] = 32;
icoDir[2] = 0; icoDir[3] = 0;
icoDir.writeUInt16LE(1, 4);
icoDir.writeUInt16LE(32, 6);
icoDir.writeUInt32LE(ico32.length, 8);
icoDir.writeUInt32LE(6 + 16, 12);
fs.writeFileSync(path.join(TARGET_DIR, 'icon.ico'), Buffer.concat([icoHeader, icoDir, ico32]));

// Minimal ICNS (for macOS)
const icnsPng = makePng(128);
const icnsInner = Buffer.concat([
  Buffer.from('ic07'),
  Buffer.alloc(4),
  icnsPng
]);
icnsInner.writeUInt32BE(icnsPng.length + 8, 4);
const icns = Buffer.concat([Buffer.from('icns'), Buffer.alloc(4), icnsInner]);
icns.writeUInt32BE(icns.length, 4);
fs.writeFileSync(path.join(TARGET_DIR, 'icon.icns'), icns);

console.log('Icons generated in', TARGET_DIR);

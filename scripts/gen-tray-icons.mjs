import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';

const SIZE = 32;
const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(__dirname, '..', 'app', 'src-tauri', 'icons', 'tray');
mkdirSync(OUT, { recursive: true });

function blank() {
  const png = new PNG({ width: SIZE, height: SIZE });
  for (let i = 0; i < png.data.length; i += 4) {
    png.data[i] = 0; png.data[i+1] = 0; png.data[i+2] = 0; png.data[i+3] = 0;
  }
  return png;
}
function plot(png, x, y) {
  if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const i = (y * SIZE + x) * 4;
  png.data[i] = 0; png.data[i+1] = 0; png.data[i+2] = 0; png.data[i+3] = 255;
}
function circleOutline(p, cx, cy, r) {
  for (let a = 0; a < 360; a += 2) {
    const rad = a * Math.PI / 180;
    plot(p, Math.round(cx + r * Math.cos(rad)), Math.round(cy + r * Math.sin(rad)));
  }
}
function circleFilled(p, cx, cy, r) {
  for (let y = -r; y <= r; y++) for (let x = -r; x <= r; x++)
    if (x*x + y*y <= r*r) plot(p, cx+x, cy+y);
}
function save(p, name) { writeFileSync(join(OUT, name), PNG.sync.write(p)); console.log('wrote', name); }

{ const p = blank(); circleOutline(p, 16, 16, 9); save(p, 'tray-loading.png'); }
{ const p = blank(); circleOutline(p, 16, 16, 9); circleOutline(p, 16, 16, 8); save(p, 'tray-ready.png'); }
{ const p = blank(); circleFilled(p, 16, 16, 8); save(p, 'tray-recording-1.png'); }
{ const p = blank(); circleFilled(p, 16, 16, 6); save(p, 'tray-recording-2.png'); }
{ const p = blank();
  for (let y = 5; y <= 19; y++) { plot(p, 15, y); plot(p, 16, y); }
  for (let dy = 22; dy <= 25; dy++) for (let dx = 14; dx <= 17; dx++) plot(p, dx, dy);
  save(p, 'tray-error.png');
}

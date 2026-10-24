// Crops and downsizes real screenshots to JPEG data URIs with headless Chrome (no dependencies). One browser run per call.
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

import { findChrome } from './chrome.mjs';

/** tasks: [{ src, x, y, w, h, outW, q? }] in source pixels; returns data URIs in the same order. */
export function crops(tasks) {
  if (!tasks.length) return [];
  const dir = mkdtempSync(join(tmpdir(), 'paddock-shots-'));
  const page = join(dir, 'crop.html');
  const srcs = [...new Set(tasks.map((t) => t.src))];
  writeFileSync(page, `<!doctype html><body>${srcs.map((s, i) => `<img id="i${i}" src="file://${s}">`).join('')}<pre id="o"></pre><script>
const T=${JSON.stringify(tasks)};const S=${JSON.stringify(srcs)};
window.addEventListener('load',()=>{const out=[];
for(const t of T){const im=document.getElementById('i'+S.indexOf(t.src));
let c=document.createElement('canvas');c.width=t.w;c.height=t.h;c.getContext('2d').drawImage(im,t.x,t.y,t.w,t.h,0,0,t.w,t.h);
const oh=Math.round(t.outW*t.h/t.w);
while(c.width>t.outW){const nw=Math.max(t.outW,Math.round(c.width/2)),nh=Math.max(oh,Math.round(c.height/2));const n=document.createElement('canvas');n.width=nw;n.height=nh;
const g=n.getContext('2d');g.imageSmoothingQuality='high';g.drawImage(c,0,0,nw,nh);c=n;}
out.push(c.toDataURL('image/jpeg',t.q||0.8));}
document.getElementById('o').textContent='@@'+JSON.stringify(out)+'@@';});</script></body>`);
  const dom = execFileSync(findChrome(), ['--headless=new', '--disable-gpu', '--allow-file-access-from-files', '--dump-dom', `file://${page}`],
    { encoding: 'utf8', maxBuffer: 1 << 29, stdio: ['ignore', 'pipe', 'ignore'] });
  const m = dom.match(/@@(\[[\s\S]*\])@@/);
  if (!m) throw new Error('screenshot crop failed (check the CHROME browser)');
  return JSON.parse(m[1]);
}

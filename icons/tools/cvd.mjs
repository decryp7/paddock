// CVD check: Machado et al. 2009 (severity 1.0) simulation in linear sRGB, distance in OKLab (x100).
const M = {
  normal: [[1,0,0],[0,1,0],[0,0,1]],
  protan: [[0.152286,1.052583,-0.204868],[0.114503,0.786281,0.099216],[-0.003882,-0.048116,1.051998]],
  deutan: [[0.367322,0.860646,-0.227968],[0.280085,0.672501,0.047413],[-0.011820,0.042940,0.968881]],
  tritan: [[1.255528,-0.076749,-0.178779],[-0.078411,0.930809,0.147602],[0.004733,0.691367,0.303900]],
};
const lin = c => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const hex = h => [1,3,5].map(i => lin(parseInt(h.slice(i, i + 2), 16) / 255));
const mul = (m, v) => m.map(r => Math.min(1, Math.max(0, r[0]*v[0] + r[1]*v[1] + r[2]*v[2])));
function oklab([r,g,b]) {
  const l = Math.cbrt(0.4122214708*r + 0.5363325363*g + 0.0514459929*b);
  const m = Math.cbrt(0.2119034982*r + 0.6806995451*g + 0.1073969566*b);
  const s = Math.cbrt(0.0883024619*r + 0.2817188376*g + 0.6299787005*b);
  return [0.2104542553*l + 0.7936177850*m - 0.0040720468*s, 1.9779984951*l - 2.4285922050*m + 0.4505937099*s, 0.0259040371*l + 0.7827717662*m - 0.8086757660*s];
}
const dist = (a, b) => 100 * Math.hypot(a[0]-b[0], a[1]-b[1], a[2]-b[2]);
const lum = h => { const [r,g,b] = hex(h); return 0.2126*r + 0.7152*g + 0.0722*b; };
export const contrast = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
export function check(palette) {
  const names = Object.keys(palette); const out = {};
  for (const [k, m] of Object.entries(M)) {
    let min = Infinity, pair = '';
    for (let i = 0; i < names.length; i++) for (let j = i + 1; j < names.length; j++) {
      const d = dist(oklab(mul(m, hex(palette[names[i]]))), oklab(mul(m, hex(palette[names[j]]))));
      if (d < min) { min = d; pair = `${names[i]}~${names[j]}`; }
    }
    out[k] = { minDeltaE: +min.toFixed(1), closest: pair };
  }
  return out;
}
if (process.argv[1] && process.argv[1].endsWith("cvd.mjs")) {
  const sets = JSON.parse(process.argv[2]);
  for (const [n, p] of Object.entries(sets)) console.log(n, JSON.stringify(check(p)), Object.fromEntries(Object.entries(p).map(([k,v]) => [k, contrast(v,'#FFFFFF').toFixed(2)])));
}

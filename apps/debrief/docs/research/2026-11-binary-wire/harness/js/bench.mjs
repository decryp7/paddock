// Throwaway: decode and lookup cost per format. Bundled for browsers, also run in Node.
import { decode as mpDecode } from '@msgpack/msgpack';
import { Unpackr } from 'msgpackr';
import { Decoder as CborDecoder } from 'cbor-x';
import { toReference } from 'flatbuffers/js/flexbuffers.js';
import { ByteBuffer } from 'flatbuffers/js/byte-buffer.js';
import { tableFromIPC as arrowFromIPC } from 'apache-arrow';
import { tableFromIPC as flFromIPC } from '@uwdata/flechette';
import * as ion from 'ion-js';
import { BSON } from 'bson';

const td = new TextDecoder();
const unpackr = new Unpackr({ useRecords: false, mapsAsObjects: true });
const cbor = new CborDecoder({ useRecords: false, mapsAsObjects: true });
const ROW = 137, FROM = 80, VIS = 40;

// ---- msgpack / cbor skip scanning: the fair "lookup without decoding" for a format with no index.
function mpSkip(d, p) { const b = d.getUint8(p); p++;
  if (b < 0x80 || b >= 0xe0) return p; if (b < 0x90) { let n = (b & 15) * 2; while (n--) p = mpSkip(d, p); return p; } if (b < 0xa0) { let n = b & 15; while (n--) p = mpSkip(d, p); return p; } if (b < 0xc0) return p + (b & 31);
  switch (b) { case 0xc0: case 0xc2: case 0xc3: return p; case 0xc4: return p + 1 + d.getUint8(p); case 0xc5: return p + 2 + d.getUint16(p); case 0xc6: return p + 4 + d.getUint32(p); case 0xca: return p + 4; case 0xcb: return p + 8; case 0xcc: case 0xd0: return p + 1; case 0xcd: case 0xd1: return p + 2; case 0xce: case 0xd2: return p + 4; case 0xcf: case 0xd3: return p + 8; case 0xd9: return p + 1 + d.getUint8(p); case 0xda: return p + 2 + d.getUint16(p); case 0xdb: return p + 4 + d.getUint32(p);
    case 0xdc: { let n = d.getUint16(p); p += 2; while (n--) p = mpSkip(d, p); return p; } case 0xdd: { let n = d.getUint32(p); p += 4; while (n--) p = mpSkip(d, p); return p; } case 0xde: { let n = d.getUint16(p) * 2; p += 2; while (n--) p = mpSkip(d, p); return p; } case 0xdf: { let n = d.getUint32(p) * 2; p += 4; while (n--) p = mpSkip(d, p); return p; } }
  throw new Error('mp ' + b); }
function mpStr(d, p, u8) { const b = d.getUint8(p); let n, q; if (b >= 0xa0 && b < 0xc0) { n = b & 31; q = p + 1; } else if (b === 0xd9) { n = d.getUint8(p + 1); q = p + 2; } else if (b === 0xda) { n = d.getUint16(p + 1); q = p + 3; } else { n = d.getUint32(p + 1); q = p + 5; } return [td.decode(u8.subarray(q, q + n)), q + n]; }
function mpLen(d, p) { const b = d.getUint8(p); if ((b & 0xf0) === 0x80 || (b & 0xf0) === 0x90) return [b & 15, p + 1]; if (b === 0xdc || b === 0xde) return [d.getUint16(p + 1), p + 3]; return [d.getUint32(p + 1), p + 5]; }
function mpField(d, p, u8, key) { let [n, q] = mpLen(d, p); while (n--) { const [k, v] = mpStr(d, q, u8); if (k === key) return v; q = mpSkip(d, v); } return -1; }
function mpLookup(u8, row) { const d = new DataView(u8.buffer, u8.byteOffset, u8.byteLength); let p = mpField(d, 0, u8, 'rows'); let [, q] = mpLen(d, p); for (let i = 0; i < row; i++) q = mpSkip(d, q); return mpStr(d, mpField(d, q, u8, 'message'), u8)[0]; }

// ---- FlatBuffers by hand (flatc not run; same runtime calls its output makes).
const fbRoot = (bb) => bb.readInt32(bb.position()) + bb.position();
const fbStr = (bb, t, slot) => { const o = bb.__offset(t, 4 + 2 * slot); return o ? bb.__string(t + o) : null; };
function fbRow(bb, root, i) { const o = bb.__offset(root, 8); return bb.__indirect(bb.__vector(root + o) + i * 4); }
const fbRows = (bb, root) => bb.__vector_len(root + bb.__offset(root, 8));
function fbFieldVal(bb, row, key) { const o = bb.__offset(row, 4 + 2 * 7); if (!o) return null; const len = bb.__vector_len(row + o), v = bb.__vector(row + o); for (let i = 0; i < len; i++) { const f = bb.__indirect(v + i * 4); if (fbStr(bb, f, 0) === key) return fbStr(bb, f, 1); } return null; }
function fbFullRow(bb, t) { const scalar = (s, rd, d) => { const o = bb.__offset(t, 4 + 2 * s); return o ? rd(t + o) : d; }; const fields = {}; const fo = bb.__offset(t, 18); if (fo) { const len = bb.__vector_len(t + fo), v = bb.__vector(t + fo); for (let i = 0; i < len; i++) { const f = bb.__indirect(v + i * 4); fields[fbStr(bb, f, 0)] = fbStr(bb, f, 1); } }
  const markers = []; const mo = bb.__offset(t, 30); if (mo) { const len = bb.__vector_len(t + mo), v = bb.__vector(t + mo); for (let i = 0; i < len; i++) markers.push(bb.__string(v + i * 4)); }
  return { id: scalar(0, (p) => bb.readUint32(p), 0), ts: Number(scalar(1, (p) => bb.readInt64(p), 0n)), level: scalar(3, (p) => bb.readUint8(p), 0), message: fbStr(bb, t, 4), template: fbStr(bb, t, 5), exception: fbStr(bb, t, 6), fields, source: scalar(8, (p) => bb.readUint32(p), 0), sourceName: fbStr(bb, t, 9), line: scalar(10, (p) => bb.readUint32(p), 0), byteOffset: Number(scalar(11, (p) => bb.readUint64(p), 0n)), group: fbStr(bb, t, 12), markers }; }
// Generic Frame
function frOpen(u8) { const bb = new ByteBuffer(u8); const root = fbRoot(bb); const o = bb.__offset(root, 6); const n = bb.__vector_len(root + o), v = bb.__vector(root + o); const cols = {}; for (let i = 0; i < n; i++) { const c = bb.__indirect(v + i * 4); const ty = bb.readUint8(c + bb.__offset(c, 6)); const dataT = bb.__indirect(c + bb.__offset(c, 8)); cols[fbStr(bb, c, 0)] = [ty, dataT]; } return { bb, u8, cols, rows: bb.readUint32(root + bb.__offset(root, 4)) }; }
function frGet(fr, name, i) { const [ty, t] = fr.cols[name]; const bb = fr.bb; const vec = (s) => { const o = bb.__offset(t, 4 + 2 * s); return [bb.__vector(t + o), bb.__vector_len(t + o)]; }; if (ty === 1) { const [v] = vec(0); return bb.readUint32(v + i * 4); } if (ty === 2) { const [v] = vec(0); return Number(bb.readInt64(v + i * 8)); } if (ty === 3) { const [ov] = vec(0), [bv] = vec(1); const a = bb.readUint32(ov + i * 4), z = bb.readUint32(ov + (i + 1) * 4); return td.decode(fr.u8.subarray(bv + a, bv + z)); } const [kv] = vec(0), [sv] = vec(1); return bb.__string(sv + bb.readUint8(kv + i) * 4); }

// ---- postcard: a positional reader as a generator would write it.
function pcReader(u8) { let p = 0; const vi = () => { let r = 0, m = 1, b; do { b = u8[p++]; r += (b & 127) * m; m *= 128; } while (b & 128); return r; }; const zz = () => { const v = vi(); return v % 2 ? -(v + 1) / 2 : v / 2; }; const str = () => { const n = vi(); const s = td.decode(u8.subarray(p, p + n)); p += n; return s; }; const skipS = () => { const n = vi(); p += n; }; const opt = (f) => (u8[p++] ? f() : null); const row = () => ({ id: vi(), ts: opt(zz), level: u8[p++], message: str(), template: opt(str), exception: opt(str), fields: JSON.parse(str()), source: vi(), sourceName: str(), line: vi(), byteOffset: vi(), group: str(), markers: Array.from({ length: vi() }, str) }); const skipRow = () => { vi(); if (u8[p++]) vi(); p++; skipS(); if (u8[p++]) skipS(); if (u8[p++]) skipS(); skipS(); vi(); skipS(); vi(); vi(); skipS(); let m = vi(); while (m--) skipS(); }; return { vi, opt, row, skipRow }; }

// ---- full materialisation helpers (touch every value, as today's JSON does in parse).
function fullFlex(u8) { return toReference(u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength)).toObject(); }
function arrowRows(t) { return t.toArray().map((r) => (r.toJSON ? r.toJSON() : r)); }
const COLS = ['ts', 'level', 'message', 'sourceName', 'group'];
function arrowVisible(t) { const cs = COLS.map((c) => t.getChild(c)); const f = t.getChild('fields'); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { for (const c of cs) { const v = c.get(i); n += v == null ? 0 : 1; } const fj = JSON.parse(f.get(i)); n += fj.connectionId === undefined ? 0 : 1; } return n; }
function flVisible(t) { const cs = COLS.map((c) => t.getChild(c)); const f = t.getChild('fields'); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { for (const c of cs) { const v = c.at(i); n += v == null ? 0 : 1; } const fj = JSON.parse(f.at(i)); n += fj.connectionId === undefined ? 0 : 1; } return n; }
function flVisibleDyn(t) { const cs = [...COLS, 'f.connectionId'].map((c) => t.getChild(c)); let n = 0; for (let i = FROM; i < FROM + VIS; i++) for (const c of cs) { const v = c.at(i); n += v == null ? 0 : 1; } return n; }
function flVisibleMap(t) { const cs = COLS.map((c) => t.getChild(c)); const f = t.getChild('fields'); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { for (const c of cs) { const v = c.at(i); n += v == null ? 0 : 1; } for (const [k] of f.at(i)) if (k === 'connectionId') n++; } return n; }
function objVisible(o) { const rows = o.rows; let n = 0; for (let i = FROM; i < FROM + VIS; i++) { const r = rows[i]; for (const c of COLS) n += r[c] == null ? 0 : 1; n += r.fields.connectionId === undefined ? 0 : 1; } return n; }

export const formats = {
  json: { file: 'json', full: (u) => JSON.parse(td.decode(u)), one: (u) => JSON.parse(td.decode(u)).rows[ROW].message, visible: (u) => objVisible(JSON.parse(td.decode(u))) },
  msgpack: { file: 'msgpack', full: (u) => mpDecode(u), one: (u) => mpDecode(u).rows[ROW].message, visible: (u) => objVisible(mpDecode(u)) },
  'msgpack scan': { file: 'msgpack', one: (u) => mpLookup(u, ROW) },
  msgpackr: { file: 'msgpack', full: (u) => unpackr.unpack(u), one: (u) => unpackr.unpack(u).rows[ROW].message, visible: (u) => objVisible(unpackr.unpack(u)) },
  'cbor-x': { file: 'cbor', full: (u) => cbor.decode(u), one: (u) => cbor.decode(u).rows[ROW].message, visible: (u) => objVisible(cbor.decode(u)) },
  postcard: { file: 'postcard', full: (u) => { const r = pcReader(u); const total = r.vi(), offset = r.vi(); const n = r.vi(); const rows = new Array(n); for (let i = 0; i < n; i++) rows[i] = r.row(); return { total, offset, rows }; }, one: (u) => { const r = pcReader(u); r.vi(); r.vi(); r.vi(); for (let i = 0; i < ROW; i++) r.skipRow(); return r.row().message; } },
  flexbuffers: { file: 'flex', full: fullFlex,
    one: (u) => toReference(u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength)).get('rows').get(ROW).get('message').stringValue(),
    visible: (u) => { const rows = toReference(u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength)).get('rows'); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { const r = rows.get(i); for (const c of COLS) n += r.get(c).isNull() ? 0 : 1; const f = r.get('fields'); try { n += f.get('connectionId').isNull() ? 0 : 1; } catch {} r.get('message').stringValue(); r.get('group').stringValue(); r.get('sourceName').stringValue(); r.get('level').stringValue(); } return n; } },
  'flatbuffers per-message': { file: 'fb', full: (u) => { const bb = new ByteBuffer(u); const root = fbRoot(bb); const n = fbRows(bb, root); const rows = new Array(n); for (let i = 0; i < n; i++) rows[i] = fbFullRow(bb, fbRow(bb, root, i)); return { rows }; },
    one: (u) => { const bb = new ByteBuffer(u); return fbStr(bb, fbRow(bb, fbRoot(bb), ROW), 4); },
    visible: (u) => { const bb = new ByteBuffer(u); const root = fbRoot(bb); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { const t = fbRow(bb, root, i); const o = bb.__offset(t, 6); n += o ? 1 : 0; n += bb.readUint8(t + bb.__offset(t, 10)) >= 0 ? 1 : 0; n += fbStr(bb, t, 4) ? 1 : 0; n += fbStr(bb, t, 9) ? 1 : 0; n += fbStr(bb, t, 12) ? 1 : 0; n += fbFieldVal(bb, t, 'connectionId') ? 1 : 0; } return n; } },
  'flatbuffers generic Frame': { file: 'frame', full: (u) => { const fr = frOpen(u); const names = Object.keys(fr.cols); const rows = new Array(fr.rows); for (let i = 0; i < fr.rows; i++) { const r = {}; for (const c of names) r[c] = frGet(fr, c, i); r.fields = JSON.parse(r.fields); rows[i] = r; } return { rows }; },
    one: (u) => frGet(frOpen(u), 'message', ROW),
    visible: (u) => { const fr = frOpen(u); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { for (const c of COLS) n += frGet(fr, c, i) == null ? 0 : 1; n += JSON.parse(frGet(fr, 'fields', i)).connectionId === undefined ? 0 : 1; } return n; } },
  'arrow apache-arrow dict': { file: 'arrow_dict', full: (u) => arrowRows(arrowFromIPC(u)), one: (u) => arrowFromIPC(u).getChild('message').get(ROW), visible: (u) => arrowVisible(arrowFromIPC(u)) },
  'arrow flechette dict': { file: 'arrow_dict', full: (u) => flFromIPC(u).toArray(), one: (u) => flFromIPC(u).getChild('message').at(ROW), visible: (u) => flVisible(flFromIPC(u)) },
  'arrow flechette no dict': { file: 'arrow_plain', full: (u) => flFromIPC(u).toArray(), one: (u) => flFromIPC(u).getChild('message').at(ROW), visible: (u) => flVisible(flFromIPC(u)) },
  'arrow flechette Map fields': { file: 'arrow_map', full: (u) => flFromIPC(u).toArray(), one: (u) => flFromIPC(u).getChild('message').at(ROW), visible: (u) => flVisibleMap(flFromIPC(u)) },
  'arrow flechette useProxy': { file: 'arrow_dict', full: (u) => flFromIPC(u, { useProxy: true }).toArray(), one: (u) => flFromIPC(u, { useProxy: true }).at(ROW).message, visible: (u) => { const t = flFromIPC(u, { useProxy: true }); let n = 0; for (let i = FROM; i < FROM + VIS; i++) { const r = t.at(i); for (const c of COLS) n += r[c] == null ? 0 : 1; n += JSON.parse(r.fields).connectionId === undefined ? 0 : 1; } return n; } },
  'arrow flechette per-field cols': { file: 'arrow_dyn', full: (u) => flFromIPC(u).toArray(), one: (u) => flFromIPC(u).getChild('message').at(ROW), visible: (u) => flVisibleDyn(flFromIPC(u)) },
};
// Encoded in JS from the JSON bytes (no Rust encoder in the harness).
export function derived(jsonU8) { const obj = JSON.parse(td.decode(jsonU8)); return { bson: BSON.serialize(obj), ion: ion.dumpBinary(ionSafe(obj)) }; }
function ionSafe(o) { return o; }
export const derivedFormats = {
  bson: { full: (u) => BSON.deserialize(u), one: (u) => { const els = BSON.onDemand.parseToElements(u); const rows = els.find((e) => BSON.onDemand.ByteUtils.toUTF8(u, e[1], e[1] + e[2]) === 'rows'); const start = rows[3], arr = BSON.onDemand.parseToElements(u, start); const row = arr[ROW]; const rowEls = BSON.onDemand.parseToElements(u, row[3]); const m = rowEls.find((e) => BSON.onDemand.ByteUtils.toUTF8(u, e[1], e[1] + e[2]) === 'message'); return BSON.onDemand.ByteUtils.toUTF8(u, m[3] + 4, m[3] + m[4] - 1); } },
  'ion-js': { full: (u) => ion.load(u), one: (u) => { const r = ion.makeReader(u); r.next(); r.stepIn(); let t; while ((t = r.next())) { if (r.fieldName() === 'rows') { r.stepIn(); for (let i = 0; i <= ROW; i++) r.next(); r.stepIn(); while ((t = r.next())) { if (r.fieldName() === 'message') return r.stringValue(); } } } } },
};

// Timing: batches until 60 ms or the budget, because WebKit's clock is coarse.
export function timeIt(f, budget = 400) { const samples = []; const t0 = performance.now(); let total = 0; while (performance.now() - t0 < budget && samples.length < 30) { let n = 0; const s = performance.now(); let e = s; while (e - s < 60) { f(); n++; e = performance.now(); } samples.push((e - s) / n); total += n; } samples.sort((a, b) => a - b); return +samples[samples.length >> 1].toFixed(4); }

export function bytesOf(buf) { return new Uint8Array(buf); }

export async function runAll(load, log) { const out = {};
  for (const tag of ['200', '5000']) { const jsonU8 = await load(`page${tag}.json`); const d = derived(jsonU8); out[`page${tag}.bson.bytes`] = d.bson.byteLength; out[`page${tag}.ion.bytes`] = d.ion.byteLength;
    for (const [name, fm] of Object.entries(formats)) { const u = await load(`page${tag}.${fm.file}`); const row = {}; for (const k of ['full', 'visible', 'one']) { if (!fm[k]) continue; try { row[k] = timeIt(() => fm[k](u)); } catch (e) { row[k] = 'ERR ' + (e && e.message || e); } } out[`page${tag} ${name}`] = row; log && log(`page${tag} ${name}`, row); }
    for (const [name, fm] of Object.entries(derivedFormats)) { const u = d[name === 'bson' ? 'bson' : 'ion']; const row = {}; for (const k of ['full', 'one']) { try { row[k] = timeIt(() => fm[k](u)); } catch (e) { row[k] = 'ERR ' + (e && e.message || e); } } out[`page${tag} ${name}`] = row; log && log(`page${tag} ${name}`, row); } }
  // Facets
  { const row = {}; const j = await load('facet.json'), m = await load('facet.msgpack'), c = await load('facet.cbor'), f = await load('facet.flex'), a = await load('facet.arrow');
    row['json parse'] = timeIt(() => JSON.parse(td.decode(j))); row['msgpack'] = timeIt(() => mpDecode(m)); row['msgpackr'] = timeIt(() => unpackr.unpack(m)); row['cbor-x'] = timeIt(() => cbor.decode(c)); row['flex toObject'] = timeIt(() => fullFlex(f)); row['flechette toArray'] = timeIt(() => flFromIPC(a).toArray()); row['apache-arrow toArray'] = timeIt(() => arrowRows(arrowFromIPC(a)));
    row['flechette value[500]'] = timeIt(() => flFromIPC(a).getChild('value').at(500)); row['flex value[500]'] = timeIt(() => toReference(f.buffer.slice(f.byteOffset, f.byteOffset + f.byteLength)).get('values').get(500).get('value').stringValue()); out.facet = row; log && log('facet', row); }
  // Lanes: get a Uint32Array of 480,000 counts
  { const row = {}; const j = await load('lanes.json'), mb = await load('lanes.msgpack_bin'), ma = await load('lanes.msgpack_arr'), fx = await load('lanes.flex_bin'), a = await load('lanes.arrow'), fb = await load('lanes.fb'); const check = (v) => { if (v.length !== 480000) throw new Error('len ' + v.length); return v; };
    row['json to number[]'] = timeIt(() => check(JSON.parse(td.decode(j)).counts)); row['json then Uint32Array'] = timeIt(() => check(Uint32Array.from(JSON.parse(td.decode(j)).counts))); row['msgpack number[]'] = timeIt(() => check(mpDecode(ma).counts)); row['msgpackr number[]'] = timeIt(() => check(unpackr.unpack(ma).counts));
    row['msgpack bin + aligned copy'] = timeIt(() => { const b = mpDecode(mb).counts; return check(new Uint32Array(b.slice().buffer)); });
    row['flex blob + aligned copy'] = timeIt(() => { const b = toReference(fx.buffer.slice(fx.byteOffset, fx.byteOffset + fx.byteLength)).get('counts').blobValue(); return check(new Uint32Array(b.slice().buffer)); });
    const fbv = () => { const bb = new ByteBuffer(fb); const root = fbRoot(bb); const o = bb.__offset(root, 4); return check(new Uint32Array(fb.buffer, fb.byteOffset + bb.__vector(root + o), bb.__vector_len(root + o))); }; try { row['flatbuffers view'] = timeIt(fbv); } catch (e) { row['flatbuffers view'] = 'ERR ' + e.message; }
    row['apache-arrow toArray'] = timeIt(() => check(arrowFromIPC(a).getChild('counts').toArray())); row['flechette toArray'] = timeIt(() => check(flFromIPC(a).getChild('counts').toArray()));
    const v1 = arrowFromIPC(a).getChild('counts').toArray(), v2 = flFromIPC(a).getChild('counts').toArray(); out.lanesZeroCopy = { apacheArrowSharesBuffer: v1.buffer === a.buffer, flechetteSharesBuffer: v2.buffer === a.buffer, flechetteType: v2.constructor.name, apacheType: v1.constructor.name }; out.lanes = row; log && log('lanes', row, out.lanesZeroCopy); }
  // Settings and error
  { const row = {}; const j = await load('settings.json'), m = await load('settings.msgpack'), c = await load('settings.cbor'), f = await load('settings.flex'), a = await load('settings.arrow_json');
    row['json parse'] = timeIt(() => JSON.parse(td.decode(j))); row['msgpack'] = timeIt(() => mpDecode(m)); row['msgpackr'] = timeIt(() => unpackr.unpack(m)); row['cbor-x'] = timeIt(() => cbor.decode(c)); row['flex toObject'] = timeIt(() => fullFlex(f)); row['flechette 1 row JSON text'] = timeIt(() => JSON.parse(flFromIPC(a).getChild('json').at(0)));
    row['flex one field'] = timeIt(() => toReference(f.buffer.slice(f.byteOffset, f.byteOffset + f.byteLength)).get('sections').get(7).get('fields').get(3).get('label').stringValue()); row['msgpack one field (decode all)'] = timeIt(() => mpDecode(m).sections[7].fields[3].label); out.settings = row; log && log('settings', row); }
  { const row = {}; const j = await load('error.json'), m = await load('error.msgpack'), f = await load('error.flex'), a = await load('error.arrow');
    row['json parse'] = timeIt(() => JSON.parse(td.decode(j))); row['msgpack'] = timeIt(() => mpDecode(m)); row['msgpackr'] = timeIt(() => unpackr.unpack(m)); row['flex toObject'] = timeIt(() => fullFlex(f)); row['flex one field'] = timeIt(() => toReference(f.buffer.slice(f.byteOffset, f.byteOffset + f.byteLength)).get('message').stringValue()); row['flechette 1 row'] = timeIt(() => flFromIPC(a).toArray()[0]); row['apache-arrow 1 row'] = timeIt(() => arrowFromIPC(a).get(0).toJSON()); out.error = row; log && log('error', row); }
  return out; }

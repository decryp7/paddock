// Stylised mock screens for apps that have no screenshots yet (Sighter, Pacenotes) or whose screenshots are out of date (Dyno). Drawn in the Paddock design language
// with theme variables, in a fixed design size: hero 960x600, panel 672x280. They are concepts, not product screens.
const r = (x, y, w, h, c = 'mc', rx = 8) => `<rect class="${c}" x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}"/>`;
const t = (x, y, s, c = 'mt', a = '') => `<text class="${c}" x="${x}" y="${y}"${a ? ` text-anchor="${a}"` : ''}>${s}</text>`;
const sk = (x, y, w, c = 'ms') => r(x, y, w, 8, c, 4);
const sidebar = (items, active, h = 600, w = 196) =>
  r(0, 0, w, h, 'mside', 0) + r(w - 1, 0, 1, h, 'mrule', 0) +
  items.map((s, i) => (i === active ? r(12, 64 + i * 40, w - 24, 32, 'mact', 8) : '') + r(26, 76 + i * 40, 10, 10, i === active ? 'maccf' : 'ms', 3) +
    t(48, 86 + i * 40, s, i === active ? 'mta' : 'mm')).join('') + r(24, 22, 22, 22, 'maccf', 6) + t(56, 39, '', 'mt');

function uiFake(x, y, w, h) { // a screenshot of "some app" to detect elements in
  return r(x, y, w, h, 'mc', 10) + r(x, y, w, 30, 'mside', 10) + sk(x + 14, y + 11, 70) +
    sk(x + 24, y + 56, 90) + r(x + 24, y + 74, w - 48, 30, 'mfield', 6) +
    sk(x + 24, y + 124, 70) + r(x + 24, y + 142, w - 48, 30, 'mfield', 6) +
    r(x + w - 232, y + h - 56, 100, 32, 'mfield', 6) + r(x + w - 120, y + h - 56, 96, 32, 'maccf', 6);
}
function box(x, y, w, h, label) {
  const lw = label.length * 7.4 + 16;
  return `<rect class="mbox" x="${x}" y="${y}" width="${w}" height="${h}" rx="5"/>` + r(x, y - 22, lw, 20, 'maccf', 5) + t(x + 8, y - 8, label, 'mtw');
}


// ---- Dyno (read-only monitor): no action buttons anywhere, only readings, reasons and alarms ----
/** A sparkline through values 0..1 in the box (x, y, w, h), with a soft fill under it. */
function spark(x, y, w, h, v) {
  const P = v.map((q, i) => [x + (i * w) / (v.length - 1), y + h - q * h]);
  const d = P.map(([a, b], i) => `${i ? 'L' : 'M'}${a.toFixed(1)} ${b.toFixed(1)}`).join(' ');
  return `<path class="mfill" d="${d} L${x + w} ${y + h} L${x} ${y + h} Z"/><path class="mline" d="${d}"/>`;
}
const CPU = [0.22, 0.25, 0.21, 0.3, 0.28, 0.36, 0.52, 0.66, 0.71, 0.74, 0.7, 0.76, 0.73, 0.78];
const MEM = [0.48, 0.49, 0.5, 0.5, 0.51, 0.5, 0.52, 0.53, 0.52, 0.53, 0.54, 0.53, 0.54, 0.55];
const DSK = [0.12, 0.3, 0.18, 0.1, 0.42, 0.2, 0.14, 0.26, 0.5, 0.22, 0.16, 0.34, 0.2, 0.18];
const NET = [0.2, 0.22, 0.3, 0.26, 0.24, 0.32, 0.28, 0.35, 0.3, 0.27, 0.33, 0.29, 0.31, 0.3];
function tile(x, y, w, name, value, unit, v) {
  return r(x, y, w, 118, 'mc', 10) + t(x + 16, y + 28, name, 'mm') + t(x + 16, y + 62, value, 'mbig') + t(x + 22 + value.length * 17, y + 62, unit, 'mm') +
    spark(x + 16, y + 74, w - 32, 32, v);
}
function alarmRow(x, y, w, title, detail, state, on, cw = 108) {
  return `<circle class="${on ? 'maccf' : 'mring'}" cx="${x + 8}" cy="${y + 10}" r="5"/>` + t(x + 24, y + 14, title, 'mt') + t(x + 24, y + 34, detail, 'mm') +
    r(x + w - cw, y, cw, 24, on ? 'mact' : 'mfield', 12) + t(x + w - cw / 2, y + 16.5, state, on ? 'mta' : 'mm', 'middle');
}
function dynoHero() {
  return sidebar(['Dashboard', 'Processes', 'Why slow', 'Alarms', 'Issues', 'Counters', 'Benchmarks', 'Reports'], 0) +
    t(232, 48, 'Dashboard', 'mh') + t(232, 70, 'Watching this PC · read-only', 'mm') + r(820, 34, 108, 26, 'mact', 13) + `<circle class="maccf" cx="838" cy="47" r="4"/>` + t(850, 52, 'Monitoring', 'mta') +
    tile(232, 88, 204, 'CPU', '76', '%', CPU) + tile(446, 88, 210, 'Memory', '52', '%', MEM) + tile(672, 88, 123, 'Disk', '236', 'MB/s', DSK) + tile(805, 88, 123, 'Network', '1.2', 'MB/s', NET) +
    r(232, 222, 424, 330, 'mc', 10) + t(252, 254, 'Likeliest cause', 'mm') + t(252, 284, 'A build is using most of the CPU', 'mt2') +
    t(252, 312, 'cargo runs 6 compilers in parallel; the rest waits.', 'mm') +
    [['CPU total', '76%'], ['For', '6 min'], ['Confidence', '86%']].map(([a, b], i) => r(252 + i * 128, 330, 118, 52, 'mfield', 8) + t(264 + i * 128, 350, a, 'mm') + t(264 + i * 128, 372, b, 'mt')).join('') +
    r(252, 404, 384, 1, 'mrule', 0) + t(252, 432, 'Busiest apps', 'mm') +
    [['Rust build (cargo)', 0.82], ['Google Chrome', 0.3], ['Visual Studio Code', 0.18]].map(([a, q], i) => t(252, 462 + i * 30, a, 'mt') + r(450, 454 + i * 30, 186, 8, 'ms', 4) + r(450, 454 + i * 30, 186 * q, 8, 'maccf', 4)).join('') +
    r(672, 222, 256, 330, 'mc', 10) + t(692, 254, 'Alarms', 'mt') + t(908, 254, '2 active', 'mm', 'end') +
    alarmRow(692, 276, 216, 'Disk 78% full', 'Full in about 41 days', 'Active', true, 64) +
    alarmRow(692, 340, 216, 'CPU ran hot', '6 min above 90 °C', 'Active', true, 64) +
    alarmRow(692, 404, 216, 'Backup is old', 'Last run 9 days ago', 'Seen', false, 64) +
    r(692, 470, 216, 1, 'mrule', 0) + t(692, 500, 'Health 74 of 100', 'mt') + t(692, 522, 'Reasons, no fixes', 'mm');
}
function dynoPanel(kind) {
  if (kind === 'explain') {
    return r(40, 24, 592, 232, 'mc', 12) + t(64, 58, 'A build is using most of the CPU', 'mt2') + t(64, 84, 'cargo runs 6 compilers in parallel; everything else waits for a core.', 'mm') +
      [['CPU total', '76%'], ['For', '6 min'], ['Confidence', '86%']].map(([a, b], i) => r(64 + i * 184, 104, 172, 56, 'mfield', 8) + t(80 + i * 184, 126, a, 'mm') + t(80 + i * 184, 148, b, 'mt')).join('') +
      t(64, 192, 'Evidence', 'mm') + spark(64, 202, 544, 36, CPU);
  }
  if (kind === 'normal') {
    const x = 64, y = 64, w = 544, h = 150;
    const lo = [0.18, 0.2, 0.2, 0.22, 0.24, 0.22, 0.2, 0.2, 0.22, 0.24, 0.22, 0.2], hi = lo.map((q) => q + 0.18);
    const P = (arr) => arr.map((q, i) => [x + (i * w) / (arr.length - 1), y + h - q * h]);
    const band = P(hi).concat(P(lo).reverse()).map(([a, b], i) => `${i ? 'L' : 'M'}${a.toFixed(1)} ${b.toFixed(1)}`).join(' ') + ' Z';
    return r(40, 24, 592, 232, 'mc', 12) + t(64, 52, 'CPU, Tuesday mornings', 'mt') + t(608, 52, 'now 71% · usually 14 to 32%', 'mm', 'end') +
      `<path class="mband" d="${band}"/>` + spark(x, y, w, h, [0.24, 0.26, 0.22, 0.3, 0.28, 0.34, 0.5, 0.62, 0.7, 0.68, 0.73, 0.71]) +
      r(x, y + h, w, 1, 'mrule', 0) + t(x, y + h + 22, '7:00', 'mm') + t(x + w, y + h + 22, '11:00', 'mm', 'end') +
      r(x + 8, y + 8, 16, 10, 'mband', 3) + t(x + 32, y + 18, 'usual range', 'mm');
  }
  if (kind === 'alarms') {
    return r(40, 24, 592, 232, 'mc', 12) +
      alarmRow(64, 48, 544, 'Docker backend keeps restarting', '6 restarts in 2 hours · critical', 'Active', true) +
      alarmRow(64, 108, 544, 'Macintosh HD is 78% full', 'Full in about 41 days', 'Active', true) +
      alarmRow(64, 168, 544, 'Backup has not run for 9 days', 'Time Machine · acknowledged', 'Seen', false);
  }
  // bench
  const rows = [['Processor', 0.62, 0.69, '+11%'], ['Memory', 0.7, 0.78, '+11%'], ['Disk', 0.48, 0.54, '+12%']];
  return r(40, 24, 592, 232, 'mc', 12) + t(64, 54, 'Power mode: High performance', 'mt') + t(608, 54, 'before and after', 'mm', 'end') +
    rows.map(([n, b, a, c], i) => { const yy = 88 + i * 56;
      return t(64, yy + 14, n, 'mt') + r(176, yy, 360, 8, 'ms', 4) + r(176, yy, 360 * b, 8, 'mring2', 4) + r(176, yy + 14, 360, 8, 'ms', 4) + r(176, yy + 14, 360 * a, 8, 'maccf', 4) + t(608, yy + 18, c, 'mta', 'end'); }).join('') +
    r(176, 236, 14, 8, 'mring2', 4) + t(196, 244, 'before', 'mm') + r(260, 236, 14, 8, 'maccf', 4) + t(280, 244, 'after', 'mm');
}

export function hero(app) {
  if (app === 'dyno') return dynoHero();
  if (app === 'sighter') {
    const fx = 232, fy = 84, fw = 432, fh = 400;
    return sidebar(['Record', 'Label', 'Review', 'Train', 'Serve'], 1) +
      t(232, 52, 'Checkout screen', 'mh') + t(232, 70, '14 elements found, 2 need a label', 'mm') +
      uiFake(fx, fy, fw, fh) + box(fx + 24, fy + 74, fw - 48, 30, 'Email field  0.95') + box(fx + fw - 120, fy + fh - 56, 96, 32, 'Save  0.97') +
      box(fx + fw - 232, fy + fh - 56, 100, 32, 'Cancel  0.93') +
      r(690, 84, 238, 400, 'mc', 10) + t(708, 114, 'Detections', 'mt') +
      ['Save button', 'Cancel button', 'Email field', 'Title bar'].map((s, i) => t(708, 146 + i * 44, s, 'mm') + r(708, 154 + i * 44, 202, 6, 'ms', 3) + r(708, 154 + i * 44, 202 * [0.97, 0.93, 0.95, 0.9][i], 6, 'maccf', 3)).join('') +
      r(708, 332, 202, 1, 'mrule', 0) + t(708, 360, 'Needs labelling', 'mta') + sk(708, 376, 150) + sk(708, 396, 110) +
      r(232, 506, 696, 48, 'mc', 12) + glyphDot(256, 530) + t(276, 535, 'Find “the Save button”', 'mt') + t(912, 535, 'box 412, 288  ·  0.97  ·  detector', 'mm', 'end');
  }
  // pacenotes
  return sidebar(['Map', 'Search', 'Review', 'Diagrams', 'Requirements', 'MCP'], 0) +
    t(232, 52, 'Repository map', 'mh') + t(232, 70, 'Read-only index, nothing is executed', 'mm') +
    r(232, 84, 440, 470, 'mc', 10) + graph(232, 84, 440, 470) +
    r(692, 84, 236, 470, 'mc', 10) + t(710, 114, 'Ask', 'mt') +
    r(710, 128, 200, 52, 'mfield', 8) + t(722, 150, 'Where is the retry', 'mm') + t(722, 168, 'limit decided?', 'mm') +
    sk(710, 204, 190, 'ms') + sk(710, 222, 160) + sk(710, 240, 176) +
    ['retry.rs:42', 'config.rs:118', 'client.rs:77'].map((s, i) => r(710, 270 + i * 40, 130, 28, 'mact', 14) + t(724, 289 + i * 40, s, 'mta')).join('') +
    r(710, 410, 200, 1, 'mrule', 0) + t(710, 438, 'Callers', 'mm') + sk(710, 452, 150) + sk(710, 470, 120);
}
const glyphDot = (x, y) => `<circle class="maccf" cx="${x}" cy="${y}" r="7"/>`;
function graph(x, y, w, h) {
  const n = [[0.2, 0.18, 'main'], [0.62, 0.14, 'load_config'], [0.35, 0.42, 'Client::send'], [0.78, 0.45, 'retry_policy'], [0.18, 0.72, 'parse_args'], [0.56, 0.78, 'backoff'], [0.86, 0.8, 'Timer']];
  const e = [[0, 1], [0, 2], [1, 3], [2, 3], [2, 5], [3, 5], [4, 0], [5, 6], [3, 6]];
  const P = (i) => [x + n[i][0] * w, y + n[i][1] * h];
  return e.map(([a, b]) => { const [x1, y1] = P(a), [x2, y2] = P(b); return `<path class="medge" d="M${x1} ${y1} L${x2} ${y2}"/>`; }).join('') +
    n.map(([, , s], i) => { const [cx, cy] = P(i); const hot = i === 3; const bw = s.length * 7.6 + 24;
      return r(cx - bw / 2, cy - 15, bw, 30, hot ? 'maccf' : 'mfield', 15) + t(cx, cy + 4.5, s, hot ? 'mtw' : 'mt', 'middle'); }).join('');
}

const mono = (x, y, s, c = 'mmono') => `<text class="${c}" x="${x}" y="${y}" xml:space="preserve">${s}</text>`;
export function panel(app, kind) {
  const W = 672, H = 280, base = r(0, 0, W, H, 'mbg', 0);
  const k = `${app}:${kind}`;
  if (app === 'dyno') return base + dynoPanel(kind);
  let g = '';
  if (k === 'sighter:record') {
    g = r(40, 36, 592, 150, 'mc', 12) + `<circle class="mrec" cx="68" cy="62" r="7"/>` + t(86, 67, 'Recording  00:42', 'mt') + t(612, 67, 'Checkout, window capture', 'mm', 'end') +
      r(60, 96, 552, 1, 'mrule', 0) +
      [60, 104, 152, 232, 296, 330, 410, 470, 520, 566].map((x, i) => r(x, 112 + (i % 3) * 18, i % 2 ? 26 : 12, 10, i % 3 === 1 ? 'maccf' : 'ms', 5)).join('') +
      t(60, 176, 'click', 'mm') + t(232, 176, 'type', 'mm') + t(410, 176, 'key', 'mm') +
      [0, 1, 2, 3, 4].map((i) => r(40 + i * 120, 204, 104, 48, 'mc', 8) + r(48 + i * 120, 212, 88, 8, 'ms', 4) + r(48 + i * 120, 228, 56, 8, 'ms', 4)).join('') + r(40 + 5 * 120 - 0, 204, 0, 0, 'ms', 0);
  } else if (k === 'sighter:detect') {
    g = uiFake(150, 22, 372, 236) + box(174, 94, 324, 30, 'Email field  0.95') + box(422, 202, 76, 32, 'Save  0.97') +
      r(40, 40, 90, 28, 'mact', 14) + t(85, 59, 'OCR', 'mta', 'middle') + r(40, 80, 90, 28, 'mact', 14) + t(85, 99, 'Template', 'mta', 'middle') +
      r(40, 120, 90, 28, 'mact', 14) + t(85, 139, 'Detector', 'mta', 'middle') + r(542, 40, 90, 28, 'mact', 14) + t(587, 59, 'Vision', 'mta', 'middle');
  } else if (k === 'sighter:review') {
    g = ['Save button', 'Cancel button', 'Search field'].map((s, i) => r(40, 28 + i * 80, 592, 68, 'mc', 12) + r(56, 40 + i * 80, 76, 44, 'mfield', 6) + `<rect class="mbox" x="68" y="${50 + i * 80}" width="52" height="24" rx="4"/>` +
      t(152, 58 + i * 80, s, 'mt') + t(152, 78 + i * 80, 'suggested by detector, 0.' + (74 - i * 6), 'mm') + r(472, 46 + i * 80, 72, 32, 'maccf', 8) + t(508, 67 + i * 80, 'Accept', 'mtw', 'middle') + r(552, 46 + i * 80, 64, 32, 'mfield', 8) + t(584, 67 + i * 80, 'Skip', 'mm', 'middle')).join('');
  } else if (k === 'sighter:api') {
    g = r(40, 28, 592, 224, 'mcode', 12) + mono(64, 66, 'POST /v1/locate', 'mmonoa') + mono(64, 96, '{ "query": "the Save button" }') +
      r(64, 114, 544, 1, 'mrule', 0) + mono(64, 144, '{ "box": [412, 288, 96, 32],') + mono(64, 170, '  "confidence": 0.97,') + mono(64, 196, '  "layer": "detector" }') + t(608, 66, '127.0.0.1', 'mm', 'end');
  } else if (k === 'sighter:flaui') {
    const tag = (x, y, label, c) => r(x, y, 74, 24, c === 'uia' ? 'mact' : 'maccf', 12) + t(x + 37, y + 16.5, label, c === 'uia' ? 'mta' : 'mtw', 'middle');
    g = r(40, 28, 592, 224, 'mcode', 12) + mono(64, 58, 'Your test  (C#)', 'mmonoa') +
      mono(64, 98, 'var win = app.GetMainWindow(auto);') + tag(546, 80, 'UIA', 'uia') +
      mono(64, 130, 'win.FindFirstDescendant(ByName("Save"))') + tag(546, 112, 'UIA', 'uia') +
      mono(64, 162, 'chart.Sighter().GetByText("Pump 3")') + tag(546, 144, 'Sighter', 'sight') +
      r(64, 184, 544, 1, 'mrule', 0) + t(64, 214, 'Auto-waits, exactly one match: 0.96 via detector', 'mm') + t(608, 214, 'then ClickAsync()', 'mm', 'end');
  } else if (k === 'sighter:apps') {
    const win = (x, y, name, model, on) => r(x, y, 188, 118, on ? 'mcon' : 'mc', 10) + r(x, y, 188, 26, 'mside', 10) + sk(x + 14, y + 9, 60) +
      t(x + 14, y + 52, name, 'mt') + r(x + 14, y + 66, 120, 8, 'ms', 4) + r(x + 14, y + 82, 88, 8, 'ms', 4) + r(x + 14, y + 98, 76, 10, on ? 'maccf' : 'ms', 5) +
      t(x + 94, y + 107, '', 'mm') + r(x + 104, y + 90, 74, 20, 'mact', 10) + t(x + 141, y + 104, model, 'mta', 'middle');
    g = win(40, 32, 'Editor', 'editor@3', true) + win(242, 32, 'Browser', 'web@2', false) + win(444, 32, 'Terminal', 'term@1', false) +
      `<path class="medge" d="M232 90 L242 90 M434 90 L444 90"/>` + t(136, 176, 'focus verified before each step', 'mm', 'middle') +
      r(40, 196, 592, 44, 'mc', 12) + r(54, 208, 24, 20, 'maccf', 5) + t(90, 223, 'OS dialogs model, shared by every session', 'mt') + t(618, 223, 'file pickers, prompts', 'mm', 'end');
  } else if (k === 'pacenotes:graph') {
    g = r(40, 24, 592, 232, 'mc', 12) + graph(40, 24, 592, 232);
  } else if (k === 'pacenotes:review') {
    const rows = [['-', '    if retries > 3 {', 'del'], ['+', '    if retries > policy.max {', 'add'], [' ', '        return Err(Exhausted);', ''], [' ', '    }', '']];
    g = r(40, 28, 592, 224, 'mcode', 12) + mono(60, 58, 'client.rs', 'mmonoa') + rows.map(([s, l, c], i) => (c ? r(40, 74 + i * 28, 592, 28, c === 'add' ? 'madd' : 'mdel', 0) : '') + mono(60, 93 + i * 28, s) + mono(84, 93 + i * 28, l)).join('') +
      r(60, 194, 552, 44, 'mc', 8) + `<circle class="maccf" cx="82" cy="216" r="6"/>` + t(98, 221, 'Max retries is now unbounded when policy.max is unset', 'mm');
  } else if (k === 'pacenotes:diagram') {
    g = r(40, 24, 592, 232, 'mc', 12) + r(70, 100, 130, 60, 'mfield', 10) + t(135, 135, 'App', 'mt', 'middle') + r(270, 52, 130, 60, 'maccf', 10) + t(335, 87, 'Index', 'mtw', 'middle') +
      r(270, 168, 130, 60, 'mfield', 10) + t(335, 203, 'Store', 'mt', 'middle') + r(472, 100, 130, 60, 'mfield', 10) + t(537, 135, 'MCP server', 'mt', 'middle') +
      `<path class="medge" d="M200 120 L270 90 M200 142 L270 198 M400 82 L472 120 M400 198 L472 142 M335 112 L335 168"/>` + r(500, 36, 100, 22, 'mact', 11) + t(550, 51, 'draw.io', 'mta', 'middle');
  } else if (k === 'pacenotes:mcp') {
    g = r(40, 28, 270, 224, 'mc', 12) + t(60, 58, 'Local AI', 'mt') + sk(60, 76, 150) + sk(60, 96, 190) + sk(60, 116, 120) + r(60, 150, 230, 80, 'mfield', 8) + t(74, 176, 'Where does this', 'mm') + t(74, 196, 'exception come from?', 'mm') +
      `<path class="medge" d="M318 140 L354 140 M346 132 L354 140 L346 148"/>` +
      r(362, 28, 270, 224, 'mc', 12) + t(382, 58, 'Pacenotes tools', 'mt') + ['Resolve stack trace', 'Find references', 'Repo map', 'Read-only, redacted'].map((s, i) => r(382, 78 + i * 42, 230, 32, i === 0 ? 'maccf' : 'mact', 16) + t(397, 99 + i * 42, s, i === 0 ? 'mtw' : 'mta')).join('');
  }
  return base + g;
}
export const MOCK_CSS = `.mbg{fill:var(--soft)}.mc{fill:var(--card);stroke:var(--line)}.mside{fill:var(--bar)}.mrule{fill:var(--line)}.ms{fill:var(--line)}
.mfield{fill:var(--soft);stroke:var(--line)}.mact{fill:var(--acc);fill-opacity:.14}.maccf{fill:var(--accf)}.mbox{fill:var(--acc);fill-opacity:.12;stroke:var(--acc);stroke-width:2}
.mt{font-size:14px;font-weight:500;fill:var(--ink)}.mm{font-size:13px;fill:var(--muted)}.mta{font-size:13px;font-weight:600;fill:var(--acct)}.mtw{font-size:12.5px;font-weight:600;fill:#fff}.mh{font-size:20px;font-weight:600;fill:var(--ink)}
.medge{fill:none;stroke:var(--link);stroke-width:1.5}.mcode{fill:var(--codebg);stroke:var(--line)}.mmono{font:13px ui-monospace,"SF Mono",Menlo,Consolas,monospace;fill:var(--ink)}.mmonoa{font:600 13px ui-monospace,"SF Mono",Menlo,Consolas,monospace;fill:var(--acct)}
.madd{fill:#2EA043;fill-opacity:.18}.mdel{fill:#F85149;fill-opacity:.16}.mrec{fill:#E5484D}.mcon{fill:var(--card);stroke:var(--acc);stroke-width:2}
.mline{fill:none;stroke:var(--acc);stroke-width:2;stroke-linejoin:round}.mfill{fill:var(--acc);fill-opacity:.10}.mband{fill:var(--acc);fill-opacity:.22}
.mring{fill:none;stroke:var(--muted);stroke-width:1.5}.mring2{fill:var(--muted);fill-opacity:.45}.mbig{font-size:28px;font-weight:600;fill:var(--ink)}.mt2{font-size:18px;font-weight:600;fill:var(--ink)}`;

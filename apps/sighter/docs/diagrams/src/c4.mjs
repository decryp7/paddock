import { P, save, link } from './common.mjs';

// ---------- C4 level 2: containers (planned) ----------
const d = P({ title: 'Sighter — containers', kind: 'C4 · Level 2 — Containers · planned', w: 1584,
  desc: 'Planned design, nothing is built yet. The studio and the recorder are for people; the locator server answers tests on loopback; training runs in a sandbox.' });
const NW = 208, NH = 128, EW = 216;
const lx = 48, gx = 360, cx = [384, 688, 992], rx = 1320;
const r0 = 144, g1 = r0 + NH + 64, row = (i) => g1 + 48 + i * (NH + 56), gh = 48 + NH * 2 + 56 + 24, gw = 24 + 3 * NW + 2 * 96 + 24;

d.node('cli', cx[2], r0, NW, NH, 'accent', { name: 'Client libraries', tech: 'C#, Java, C++, Python, TypeScript', desc: 'Playwright-style locators over the HTTP API; used by engineers\' tests.' });
d.node('pit', rx, r0, EW, NH, 'external', { name: 'Pitcrew runners', tech: 'CI runners', desc: 'Run the tests and `sighter run` on a machine with a desktop session.' });

d.group('g1', gx, g1, gw, gh, 'Sighter machine [one user session, loopback only]');
d.node('studio', cx[0], row(0), NW, NH, 'ui', { name: 'Studio app', tech: 'Tauri 2, Angular', desc: 'Label, review queue, test authoring, models. Pitwall may help label.' });
d.node('train', cx[1], row(0), NW, NH, 'accent', { name: 'Training runtime', tech: 'sandboxed process, downloaded on request', desc: 'Trains the permissive YOLO-style detector from reviewed labels.' });
d.node('srv', cx[2], row(0), NW, NH, 'accent', { name: 'Locator server', tech: 'Rust, HTTP API on loopback', desc: 'Layered resolver and guarded input driver; `sighter run`.' });
d.node('rec', cx[0], row(1), NW, NH, 'accent', { name: 'Recorder', tech: 'studio and headless CLI', desc: 'Screen and input events; accessibility optional; redacts before storing.' });
d.node('store', cx[1], row(1), 2 * NW + 96, NH, 'process', { name: 'Model store and data folder', tech: 'versioned, immutable', desc: 'Recordings, labels, ONNX models (app@N) and run bundles. Local files only.' });

d.node('pn', lx, row(0), EW, NH, 'external', { name: 'Pacenotes', tech: 'sibling app', desc: 'Test plans in, results and requirement traceability out.' });
d.node('app', lx, row(1), EW, NH, 'external', { name: 'App under test', tech: 'any UI', desc: 'Captured by sight, driven only through the guarded input driver.' });
d.node('pw', rx, row(0), EW, NH, 'external', { name: 'Pitwall', tech: 'local AI hub, optional', desc: 'Vision model for grounding, labelling help, recovery diagnosis.' });
d.node('dbf', rx, row(1), EW, NH, 'external', { name: 'Debrief', tech: 'sibling app', desc: 'Opens run bundles: video, steps, screenshots, recovery transcript.' });

link(d, 'pit', 'L', 'cli', 'R', { label: 'Runs tests' });
link(d, 'cli', 'B', 'srv', 'T', { label: 'HTTP calls', at: 0.2 });
link(d, 'pn', 'R', 'studio', 'L', { label: 'Test plans' });
link(d, 'studio', 'R', 'train', 'L', { label: 'Starts' });
link(d, 'studio', 'B', 'rec', 'T', { label: 'Controls' });
link(d, 'rec', 'L', 'app', 'R', { label: 'Captures', rev: false });
link(d, 'rec', 'R', 'store', 'L', { label: 'Writes' });
link(d, 'train', 'B', 'store', 'T', { label: 'Reads, writes', t: [0.5, 0.2031] });
link(d, 'srv', 'B', 'store', 'T', { label: 'Loads models', t: [0.5, 0.7969] });
link(d, 'srv', 'R', 'pw', 'L', { label: 'Grounding', dash: true });
link(d, 'store', 'R', 'dbf', 'L', { label: 'Run bundles' });
d.legend([{ kind: 'ui', label: 'Studio' }, { kind: 'accent', label: 'Sighter container' }, { kind: 'process', label: 'Data' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Optional' }]);
save('c4-2-containers', d, g1 + gh);

// ---------- Component view: desktop automation, MCP server and targets (planned) ----------
{
  const d = P({ title: 'Sighter — desktop automation components', kind: 'C4 · Level 3 — Components · planned', w: 1328,
    desc: 'Planned design. Sighter runs on the controller PC (loopback and stdio only) and reaches the PC under test over RDP. MCP hosts and the built-in agent runner call the same tools; the safety guard sits in front of every action; one Target interface hides where frames come from and input goes.' });
  const NW = 208, NH = 120, GAP = 96;
  const gx = 48, gcx = (i) => gx + 24 + i * (NW + GAP), gw = 24 + 4 * NW + 3 * GAP + 24;
  const t0 = 144, g1 = t0 + NH + 72, y0 = g1 + 48, yI = y0 + NH + 56, yA = yI + 56 + 56, gh = yA + NH + 24 - g1;
  const yP = g1 + gh + 72;

  d.node('pw', gcx(0), t0, NW, NH, 'external', { name: 'Pitwall', tech: 'local AI hub, optional', desc: 'OpenAI-compatible chat with tool calling. Does not speak MCP.' });
  d.node('host', gcx(1), t0, NW, NH, 'external', { name: 'MCP hosts', tech: 'Claude Desktop/Code, Pacenotes, scripts', desc: 'Bring their own model; call the tools over stdio.' });
  d.node('cli', gcx(3), t0, NW, NH, 'external', { name: 'Plans and clients', tech: 'sighter run, C#, Java, Python', desc: 'Scripted automation (RPA); no LLM involved.' });

  d.group('g', gx, g1, gw, gh, 'Controller PC');
  d.node('run', gcx(0), y0, NW, NH, 'accent', { name: 'Agent runner', tech: 'sighter do "goal"', desc: 'Loop: observe, ask Pitwall, act, verify. Step and time budget.' });
  d.node('mcp', gcx(1), y0, NW, NH, 'accent', { name: 'MCP server', tech: 'stdio or loopback', desc: 'observe, locate, click, type, press, drag, launch_app, wait_for.' });
  d.node('sf', gcx(2), y0, NW, NH, 'accent', { name: 'Safety guard', tech: 'in the server, not the client', desc: 'Allow-list, confirmations, budget, stop hotkey, replay log.' });
  d.node('res', gcx(3), y0, NW, NH, 'accent', { name: 'Resolver', tech: 'detector, OCR, anchors', desc: 'Turns a frame into labelled elements with boxes.' });
  d.node('tgt', gcx(0), yI, gw - 48, 56, 'process', { name: 'Target interface', tech: 'frames() · input() · launch() · optional file transfer', desc: '' });
  d.node('loc', gcx(0), yA, NW, NH, 'accent', { name: 'Local adapter', tech: 'screenshots, SendInput', desc: 'This machine. Elevated windows need a helper.' });
  d.node('rdp', gcx(1), yA, NW, NH, 'accent', { name: 'RDP adapter', tech: 'FreeRDP library', desc: 'Embedded and headless: no mstsc window. Main path in v1.' });
  d.node('vnc', gcx(2), yA, NW, NH, 'accent', { name: 'VNC adapter', tech: 'VNC client', desc: 'Alongside RDP if cheap.' });
  d.node('kvm', gcx(3), yA, NW, NH, 'accent', { name: 'IP-KVM adapter', tech: 'PiKVM, NanoKVM, JetKVM APIs', desc: 'Optional, Later: BIOS, pre-logon and crash screens only.' });

  d.node('p1', gcx(0), yP, NW, NH, 'external', { name: 'This PC', tech: 'any local app', desc: 'Captured and driven in the user session.' });
  d.node('p2', gcx(1), yP, 2 * NW + GAP, NH, 'external', { name: 'PC under test, VM or VDI', tech: 'reached over RDP', desc: 'UAC prompts and elevated apps appear inside the session. Nothing installed.' });
  d.node('p3', gcx(3), yP, NW, NH, 'external', { name: 'Bare PC, Later', tech: 'behind an IP-KVM', desc: 'Firmware setup and crash screens.' });

  link(d, 'host', 'B', 'mcp', 'T', { label: 'Tool calls' });
  link(d, 'pw', 'B', 'run', 'T', { label: 'Chat + tools', dash: true });
  link(d, 'run', 'R', 'mcp', 'L', { label: 'Same tools' });
  link(d, 'mcp', 'R', 'sf', 'L', { label: 'Each call' });
  link(d, 'sf', 'R', 'res', 'L', { label: 'Approved' });
  link(d, 'cli', 'B', 'res', 'T', { label: 'Locate, act' });
  link(d, 'sf', 'B', 'tgt', 'T', { label: 'Input' });
  link(d, 'res', 'B', 'tgt', 'T', { label: 'Frames' });
  link(d, 'tgt', 'B', 'loc', 'T', {});
  link(d, 'tgt', 'B', 'rdp', 'T', {});
  link(d, 'tgt', 'B', 'vnc', 'T', { dash: true });
  link(d, 'tgt', 'B', 'kvm', 'T', { dash: true });
  link(d, 'loc', 'B', 'p1', 'T', {});
  link(d, 'rdp', 'B', 'p2', 'T', { label: 'RDP' });
  link(d, 'vnc', 'B', 'p2', 'T', { dash: true });
  link(d, 'kvm', 'B', 'p3', 'T', { label: 'HDMI, USB HID', dash: true });
  d.legend([{ kind: 'accent', label: 'Sighter component' }, { kind: 'process', label: 'Interface' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Optional or Later' }]);
  save('c4-3-desktop-automation', d, yP + NH);
}

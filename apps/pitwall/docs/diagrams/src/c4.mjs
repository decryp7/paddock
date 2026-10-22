import { P, save, link, hub, hubAt } from './common.mjs';

// ---------- C4 level 1 ----------
{
  const d = P({ title: 'Pitwall — system context', kind: 'C4 · Level 1 — System context', w: 1344,
    desc: 'Pitwall runs language models on this computer for the suite apps and any local AI tool. Prompts and answers never leave it; the only outbound traffic is downloads the user asked for, plus an opt-in check-only query about models already downloaded.' });
  const W = 288, y0 = 144, rh = 152, g = 88;
  d.node('owner', 48, y0, W, rh, 'process', { name: 'Machine owner', tech: 'Person', desc: 'Installs engines and models, approves apps, sets limits and presets, watches memory, usage and requests.', person: true });
  d.node('apps', 48, y0 + rh + g, W, rh, 'process', { name: 'Suite apps', tech: 'Software systems on this machine', desc: 'Debrief, Pitcrew, Spotter, Dyno: pair once, then ask for chat, embeddings and a model by name or capability.' });
  d.node('tools', 48, y0 + 2 * (rh + g), W, rh, 'process', { name: 'Other AI tools', tech: 'Software systems on this machine', desc: 'Editors, SDK scripts and chat front ends that speak the OpenAI, Anthropic or Ollama API; the pitwall command line.' });
  const left = 3 * rh + 2 * g;
  d.node('sys', 528, y0, 288, left, 'accent', { name: 'Pitwall', tech: 'Local LLM server and manager: Tauri app, windowless server, Rust core',
    desc: 'Downloads and runs engines and models, queues requests, loads a model when it fits and releases it when idle, keeps per-app keys, limits and usage history, model presets and engine pins, and answers on 127.0.0.1:11435. The app and tray manage it; closing the app never stops serving.' });
  const rx = 1008, rw = 288, rh2 = 128, rg = (left - rh2 * 4) / 3;
  const ext = [
    ['hf', 'Hugging Face', 'External system', 'Model search, file lists and GGUF files on request; with the opt-in update check, a read-only query for the repositories you already downloaded from.'],
    ['gh', 'GitHub releases', 'External system', 'Pinned llama.cpp builds, SHA-256 checked.'],
    ['sm', 'OS service manager', 'SCM · launchd · systemd', 'Starts the server at login or boot and restarts it.'],
  ];
  ext.forEach(([id, name, tech, desc], i) => d.node(id, rx, y0 + i * (rh2 + rg), rw, rh2, id === 'sm' ? 'process' : 'external', { name, tech, desc }));
  d.node('lan', rx, y0 + 3 * (rh2 + rg), rw, rh2, 'external', { name: 'Other computers', tech: 'Your LAN', desc: 'Shared use over TLS with a pinned certificate.', planned: true });
  hub(d, 'sys', 'L', 'owner', 'R', { rev: true, label: 'Manages' });
  hub(d, 'sys', 'L', 'apps', 'R', { rev: true, label: 'Pair, ask' });
  hub(d, 'sys', 'L', 'tools', 'R', { rev: true, label: 'Ask, script' });
  hubAt(d, 'sys', 'R', 'hf', 'L', 0.3, { label: 'Downloads' });
  hubAt(d, 'sys', 'R', 'hf', 'L', 0.75, { label: 'check only, opt-in', dash: true });
  hub(d, 'sys', 'R', 'gh', 'L', { label: 'Downloads' });
  hub(d, 'sys', 'R', 'sm', 'L', { rev: true, label: 'Starts' });
  hub(d, 'sys', 'R', 'lan', 'L', { rev: true, label: 'Asks' , dash: true });
  d.legend([{ kind: 'process', label: 'Person · OS · local software' }, { kind: 'accent', label: 'Pitwall' }, { kind: 'external', label: 'External system' }, { kind: 'tag', label: 'Planned' }, { kind: 'line', label: 'Uses' }]);
  save('c4-1-context', d, y0 + left);
}

// ---------- C4 level 2 ----------
{
  const d = P({ title: 'Pitwall — containers', kind: 'C4 · Level 2 — Containers', w: 1600,
    desc: 'The window and tray run in the user’s session; the windowless server keeps serving when the window closes; each loaded model is a separate engine process (ADR 0002, 0004, 0010).' });
  const y0 = 144, NH = 104, G = 32;
  const c1 = 48, w1 = 208;
  const c2 = c1 + w1 + 88, w2n = 232, g2w = w2n + 48;
  const c3 = c2 + g2w + 96, w3n = 216, g3w = 24 + 2 * w3n + 32 + 24;
  const c4 = c3 + g3w + 96, w4 = 224;
  const row = (i) => y0 + i * (88 + 24);
  const rows = 5, colH = rows * NH + (rows - 1) * G;
  // column 4 (right): apps, engines, hosts, service manager, LAN
  d.node('apps', c4, row(0), w4, 88, 'process', { name: 'Apps and tools', tech: 'Suite apps, pitwall CLI', desc: 'Any OpenAI, Anthropic or Ollama client.' });
  d.node('eng', c4, row(1), w4, 88, 'process', { name: 'Engine processes', tech: 'llama.cpp, or a pack engine', desc: 'One child per loaded model.' });
  const hy = row(2);
  d.group('hosts', c4, hy, w4, 200, 'Download hosts');
  d.node('hf', c4 + 12, hy + 40, w4 - 24, 56, 'external', { name: 'Hugging Face', tech: 'models, updates' });
  d.node('gh', c4 + 12, hy + 112, w4 - 24, 56, 'external', { name: 'GitHub releases', tech: 'llama.cpp builds' });
  d.node('sm', c4, row(2) + 200 + 24, w4, 64, 'process', { name: 'OS service manager', tech: 'SCM · launchd · systemd' });
  const lanY = row(2) + 200 + 24 + 64 + 24;
  d.node('lan', c4, lanY, w4, 80, 'external', { name: 'LAN listener', tech: 'TLS, pinned certificate', planned: true });
  const bottom = lanY + 80;
  // group 3: server hub spans the column; core and data below it
  const g3h = bottom - y0 + 24 + 16 + 112 + 24 - 16;
  d.group('g3', c3, y0, g3w, 40 + (bottom - y0) + 32 + 112 + 24 - 40 + 0, 'Server process [pitwall-server · windowless · 127.0.0.1 only]', { os: true });
  const hubH = bottom - y0 - 40 + 0;
  d.node('srv', c3 + 24, y0 + 40, g3w - 48, bottom - y0 - 40, 'accent', { name: 'pitwall-server', tech: 'Rust: axum, tokio; the only HTTP client is egress.rs',
    desc: 'Standard APIs (OpenAI, Anthropic, Ollama) and /x/v1 behind one gate: Host, Origin, key. Management API and live feed for the window. Serving: aliases, queue, load with parallel slots and slot affinity, idle release, caches for repeated prompts, answers and embeddings. Model library with update notices (opt-in, check only), engines with pins, rollback and capability gating, apps, keys and usage history, OS service integration. It reaches engines through pitwall-loopback and takes every decision from pitwall-core.' });
  const ry = y0 + 40 + (bottom - y0 - 40) + 32;
  d.node('core', c3 + 24, ry, w3n, 112, 'core', { name: 'pitwall-core', tech: 'Rust library, no I/O', desc: 'Decisions: formats, scheduler, memory, fit, slots, presets, limits, cache keys, update rules, pins.' });
  d.node('data', c3 + 24 + w3n + 32, ry, w3n, 112, 'process', { name: 'Data folder', tech: 'TOML, JSON, GGUF', desc: 'settings, apps, usage, presets, pins, notices, cache.sqlite, slots, models, engines.' });
  d.rect.g3[3] = ry + 112 + 24 - y0;
  // group 2: app process and the CLI
  const g2h = 40 + 96 + G + 176 + 24;
  d.group('g2', c2, y0, g2w, g2h, 'App process [Tauri 2 · logged-on user]');
  d.node('ui', c2 + 24, y0 + 40, w2n, 96, 'ui', { name: 'Angular UI', tech: 'Angular 22 in the OS webview', desc: 'Dashboard, models, engines, apps, usage, settings.' });
  d.node('shell', c2 + 24, y0 + 40 + 96 + G, w2n, 176, 'accent', { name: 'Rust shell and tray', tech: 'app/src-tauri', desc: 'Validates every UI argument, then calls the management API with the admin key from the OS credential store. One live-feed subscription feeds the window and the tray menu.' });
  d.node('owner', c1, y0, w1, null, 'process', { name: 'Machine owner', tech: 'Person', desc: 'Uses the window, tray and CLI.', person: true });
  d.node('cli', c1, y0 + g2h + G, w1, 136, 'accent', { name: 'pitwall CLI', tech: 'Rust, a terminal process', desc: 'status, ps, models, load, unload, pull, keys, presets, chat, bench; 127.0.0.1 only.' });
  link(d, 'owner', 'R', 'ui', 'L', { label: 'Uses' });
  link(d, 'ui', 'B', 'shell', 'T', { label: 'Commands' });
  link(d, 'owner', 'B', 'cli', 'T', { label: 'Types' });
  hub(d, 'srv', 'L', 'shell', 'R', { rev: true, label: 'Manages' });
  hub(d, 'srv', 'L', 'cli', 'R', { rev: true, label: 'Asks' });
  hub(d, 'srv', 'R', 'apps', 'L', { rev: true, label: 'Ask' });
  hub(d, 'srv', 'R', 'eng', 'L', { label: 'Loopback' });
  hub(d, 'srv', 'R', 'hosts', 'L', { label: 'Downloads' });
  hubAt(d, 'srv', 'R', 'hf', 'L', 0.5, { label: 'check only, opt-in', dash: true });
  hub(d, 'srv', 'R', 'sm', 'L', { rev: true, label: 'Starts' });
  hub(d, 'srv', 'R', 'lan', 'L', { rev: true, label: 'Asks', dash: true });
  link(d, 'srv', 'B', 'core', 'T', { label: 'Decisions', t: [0.2, 0.5] });
  link(d, 'srv', 'B', 'data', 'T', { label: 'Reads, writes', t: [0.8, 0.5] });
  d.legend([{ kind: 'ui', label: 'Angular UI' }, { kind: 'accent', label: 'Pitwall container' }, { kind: 'core', label: 'Rust core' }, { kind: 'process', label: 'Process · OS · data' }, { kind: 'external', label: 'External system' }, { kind: 'tag', label: 'Planned' }, { kind: 'line', label: 'Uses' }]);
  save('c4-2-containers', d, Math.max(d.rect.g3[1] + d.rect.g3[3], d.rect.cli[1] + d.rect.cli[3]));
}

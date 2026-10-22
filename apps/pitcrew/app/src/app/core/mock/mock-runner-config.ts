import type { ConfigEdit, ConfigIssue, ConfigPreview, ConfigView, DiffHunk, DiffLine, LogLevel, RunnerSettings, TextDiff } from '../api.types';

/*
 * The mock's stand-in for the engine's `runner_config`: a line-based reader and patcher for a gitea-runner
 * `config.yaml`, a simple checker and a line diff. It only has to look right for the demo and the tests; the Rust core
 * (serde_norway + yaml_patch) is the reference.
 */

/** The sections and keys of the runner's reference config the mock knows (anything else is a warning). */
const KNOWN: Record<string, string[]> = {
  log: ['level'],
  runner: ['file', 'capacity', 'envs', 'env_file', 'timeout', 'shutdown_timeout', 'insecure', 'fetch_timeout', 'fetch_interval', 'labels'],
  cache: ['enabled', 'dir', 'size_limit', 'host', 'port', 'external_server', 'secret'],
  container: ['network', 'privileged', 'options', 'workdir_parent', 'valid_volumes', 'docker_host', 'force_pull', 'force_rebuild'],
  host: ['workdir_parent'],
};

const LEVELS: LogLevel[] = ['trace', 'debug', 'info', 'warn', 'error'];

export function emptySettings(): RunnerSettings {
  return {
    capacity: null,
    labels: [],
    timeout: null,
    fetchInterval: null,
    fetchTimeout: null,
    envs: {},
    cacheEnabled: null,
    cacheDir: null,
    cacheSizeLimit: null,
    containerNetwork: null,
    containerPrivileged: null,
    containerOptions: null,
    containerValidVolumes: [],
    logLevel: null,
  };
}

/** A realistic `config.yaml` in the style of gitea-runner's `config.reference.yaml`, with comments. */
export function sampleConfigText(labels: string[]): string {
  const list = labels.map((l) => `    - "${l}"`).join('\n');
  return `# Example configuration file, it's safe to copy this as the default config file without any modification.

log:
  # The level of logging, can be trace, debug, info, warn, error, fatal
  level: info

runner:
  # Where to store the registration result.
  file: .runner
  # Execute how many tasks concurrently at the same time.
  capacity: 1
  # Extra environment variables to run jobs.
  envs:
    NUGET_PACKAGES: D:\\cache\\nuget
  # The timeout for a job to be finished.
  # Please note that the Gitea instance also has a timeout (3h by default) for the job.
  timeout: 3h
  # Whether skip verifying the TLS certificate of the Gitea instance.
  insecure: false
  # The timeout for fetching the job from the Gitea instance.
  fetch_timeout: 5s
  # The interval for fetching the job from the Gitea instance.
  fetch_interval: 2s
  # The labels of a runner are used to determine which jobs the runner can run, and how to run them.
  # Like: "macos-arm64:host" or "ubuntu-latest:docker://docker.gitea.com/runner-images:ubuntu-latest"
  labels:
${list}

cache:
  # Enable cache server to use actions/cache.
  enabled: true
  # The directory to store the cache data.
  dir: ""
`;
}

/** A label as written in the file's list: `windows:host`, `ubuntu-latest:docker://image`. */
export function labelText(name: string, container: boolean): string {
  return container ? `${name}:docker://gitea/runner-images:${name === 'ubuntu-latest' ? 'ubuntu-latest' : 'ubuntu-22.04'}` : `${name}:host`;
}

interface Line {
  indent: number;
  key: string | null;
  value: string;
  item: string | null;
}

function scan(text: string): Line[] {
  return text.split('\n').map((raw) => {
    const noComment = /^\s*#/.test(raw) ? '' : raw;
    const indent = raw.length - raw.trimStart().length;
    const item = /^\s*-\s+(.*)$/.exec(noComment);
    if (item) return { indent, key: null, value: '', item: unquote(item[1] ?? '') };
    const kv = /^\s*([A-Za-z0-9_.-]+):\s*(.*?)\s*$/.exec(noComment);
    return { indent, key: kv?.[1] ?? null, value: unquote(kv?.[2] ?? ''), item: null };
  });
}

function unquote(v: string): string {
  const m = /^"(.*)"$/.exec(v) ?? /^'(.*)'$/.exec(v);
  return (m ? (m[1] ?? '') : v.replace(/\s+#.*$/, '')).trim();
}

/** Reads the form's settings from the text (best effort). */
export function readSettings(text: string): RunnerSettings {
  const s = emptySettings();
  let section = '';
  let key = '';
  for (const l of scan(text)) {
    if (l.key !== null && l.indent === 0) {
      section = l.key;
      key = '';
      continue;
    }
    if (l.key !== null && l.indent <= 2) {
      key = l.key;
      const v = l.value;
      const path = `${section}.${key}`;
      if (v === '' || v === '[]') continue;
      switch (path) {
        case 'runner.capacity':
          s.capacity = Number.isFinite(Number(v)) ? Math.trunc(Number(v)) : null;
          break;
        case 'runner.timeout':
          s.timeout = v;
          break;
        case 'runner.fetch_interval':
          s.fetchInterval = v;
          break;
        case 'runner.fetch_timeout':
          s.fetchTimeout = v;
          break;
        case 'cache.enabled':
          s.cacheEnabled = v === 'true';
          break;
        case 'cache.size_limit':
          s.cacheSizeLimit = v;
          break;
        case 'container.network':
          s.containerNetwork = v;
          break;
        case 'container.privileged':
          s.containerPrivileged = v === 'true';
          break;
        case 'container.options':
          s.containerOptions = v;
          break;
        case 'log.level':
          s.logLevel = LEVELS.find((x) => x === v) ?? null;
          break;
      }
      if (path === 'cache.dir') s.cacheDir = v;
      continue;
    }
    const path = `${section}.${key}`;
    if (l.item !== null) {
      if (path === 'runner.labels') s.labels.push(l.item);
      if (path === 'container.valid_volumes') s.containerValidVolumes.push(l.item);
    } else if (l.key !== null && path === 'runner.envs') {
      s.envs[l.key] = l.value;
    }
  }
  return s;
}

const SCALARS: { path: string; get: (s: RunnerSettings) => string | null }[] = [
  { path: 'log.level', get: (s) => s.logLevel },
  { path: 'runner.capacity', get: (s) => (s.capacity === null ? null : String(s.capacity)) },
  { path: 'runner.timeout', get: (s) => s.timeout },
  { path: 'runner.fetch_interval', get: (s) => s.fetchInterval },
  { path: 'runner.fetch_timeout', get: (s) => s.fetchTimeout },
  { path: 'cache.enabled', get: (s) => (s.cacheEnabled === null ? null : String(s.cacheEnabled)) },
  { path: 'cache.dir', get: (s) => (s.cacheDir === null ? null : s.cacheDir === '' ? '""' : s.cacheDir) },
  { path: 'cache.size_limit', get: (s) => s.cacheSizeLimit },
  { path: 'container.network', get: (s) => s.containerNetwork },
  { path: 'container.privileged', get: (s) => (s.containerPrivileged === null ? null : String(s.containerPrivileged)) },
  { path: 'container.options', get: (s) => s.containerOptions },
];

const BLOCKS: { path: string; get: (s: RunnerSettings) => string[] }[] = [
  { path: 'runner.labels', get: (s) => s.labels.map((l) => `- "${l}"`) },
  { path: 'container.valid_volumes', get: (s) => s.containerValidVolumes.map((v) => `- "${v}"`) },
  { path: 'runner.envs', get: (s) => Object.entries(s.envs).map(([k, v]) => `${k}: ${v}`) },
];

function sectionRange(lines: string[], section: string): [number, number] | null {
  const start = lines.findIndex((l) => l === `${section}:` || l.startsWith(`${section}: `));
  if (start < 0) return null;
  let end = start + 1;
  let last = start;
  for (; end < lines.length; end++) {
    const l = lines[end] ?? '';
    if (l.trim() === '' || /^\s*#/.test(l)) continue;
    if (!/^\s/.test(l)) break;
    last = end;
  }
  return [start, last];
}

/** Index of the `  key:` line inside a section, and where its block (deeper lines) ends. */
function keyRange(lines: string[], section: string, key: string): [number, number] | null {
  const r = sectionRange(lines, section);
  if (!r) return null;
  for (let i = r[0] + 1; i <= r[1]; i++) {
    if (new RegExp(`^  ${key}:`).test(lines[i] ?? '')) {
      let end = i;
      for (let j = i + 1; j <= r[1]; j++) {
        const l = lines[j] ?? '';
        if (/^ {4,}\S/.test(l) && !/^\s*#/.test(l)) end = j;
        else if (l.trim() === '' || /^\s*#/.test(l)) continue;
        else break;
      }
      return [i, end];
    }
  }
  return null;
}

function put(lines: string[], path: string, block: string[]): void {
  const [section, key] = path.split('.') as [string, string];
  const existing = keyRange(lines, section, key);
  if (existing) {
    lines.splice(existing[0], existing[1] - existing[0] + 1, ...block);
    return;
  }
  const r = sectionRange(lines, section);
  if (r) lines.splice(r[1] + 1, 0, ...block);
  else {
    if (lines.length > 0 && lines[lines.length - 1] === '') lines.pop();
    lines.push('', `${section}:`, ...block, '');
  }
}

function remove(lines: string[], path: string): void {
  const [section, key] = path.split('.') as [string, string];
  const r = keyRange(lines, section, key);
  if (r) lines.splice(r[0], r[1] - r[0] + 1);
}

/** Applies the form's settings to the text: only keys that differ are touched, comments stay. */
export function applyForm(text: string, next: RunnerSettings): string {
  const before = readSettings(text);
  const lines = text.split('\n');
  for (const f of SCALARS) {
    const a = f.get(before);
    const b = f.get(next);
    if (a === b) continue;
    const key = f.path.split('.')[1] ?? '';
    if (b === null) remove(lines, f.path);
    else put(lines, f.path, [`  ${key}: ${b}`]);
  }
  for (const b of BLOCKS) {
    const a = b.get(before);
    const n = b.get(next);
    if (JSON.stringify(a) === JSON.stringify(n)) continue;
    const key = b.path.split('.')[1] ?? '';
    if (n.length === 0) remove(lines, b.path);
    else put(lines, b.path, [`  ${key}:`, ...n.map((x) => `    ${x}`)]);
  }
  return lines.join('\n');
}

const DURATION = /^(\d+(ms|s|m|h))+$/;

function seconds(d: string): number {
  let total = 0;
  for (const m of d.matchAll(/(\d+)(ms|s|m|h)/g)) total += Number(m[1]) * { ms: 0.001, s: 1, m: 60, h: 3600 }[m[2] as 'ms' | 's' | 'm' | 'h'];
  return total;
}

function lineOf(text: string, key: string): number | null {
  const i = text.split('\n').findIndex((l) => new RegExp(`^\\s*${key}:`).test(l));
  return i < 0 ? null : i + 1;
}

/** Findings about a text: syntax first, then unknown keys, then values. */
export function checkText(text: string): ConfigIssue[] {
  const issues: ConfigIssue[] = [];
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i] ?? '';
    if (l.startsWith('\t')) {
      issues.push({ severity: 'error', path: '', line: i + 1, message: 'Line starts with a tab. YAML indents with spaces; replace the tab with spaces.' });
    } else if (/^\s*: \[/.test(l)) {
      issues.push({ severity: 'error', path: '', line: i + 1, message: 'This line starts with ": [" and cannot be read as YAML. A key is missing before the colon.' });
    }
  }
  if (issues.some((x) => x.severity === 'error')) return issues;

  let section = '';
  lines.forEach((raw, i) => {
    if (/^\s*#/.test(raw) || raw.trim() === '') return;
    const m = /^(\s*)([A-Za-z0-9_.-]+):/.exec(raw);
    if (!m) return;
    const indent = (m[1] ?? '').length;
    const key = m[2] ?? '';
    if (indent === 0) {
      section = key;
      if (!(key in KNOWN)) issues.push({ severity: 'warning', path: key, line: i + 1, message: `Unknown key "${key}". The runner ignores it; check the spelling.` });
    } else if (indent === 2 && section in KNOWN && !(KNOWN[section] ?? []).includes(key)) {
      issues.push({ severity: 'warning', path: `${section}.${key}`, line: i + 1, message: `Unknown key "${section}.${key}". The runner ignores it; check the spelling.` });
    }
  });

  const s = readSettings(text);
  if (s.capacity !== null && (s.capacity < 1 || s.capacity > 64)) {
    issues.push({ severity: 'error', path: 'runner.capacity', line: lineOf(text, 'capacity'), message: 'Capacity must be between 1 and 64 jobs at the same time.' });
  }
  if (s.timeout !== null && !DURATION.test(s.timeout)) {
    issues.push({ severity: 'error', path: 'runner.timeout', line: lineOf(text, 'timeout'), message: `"${s.timeout}" is not a duration. Use something like 3h, 90m or 30s.` });
  }
  if (s.fetchInterval !== null && !DURATION.test(s.fetchInterval)) {
    issues.push({ severity: 'error', path: 'runner.fetch_interval', line: lineOf(text, 'fetch_interval'), message: `"${s.fetchInterval}" is not a duration. Use something like 2s.` });
  }
  if (s.fetchTimeout !== null) {
    if (!DURATION.test(s.fetchTimeout)) {
      issues.push({ severity: 'error', path: 'runner.fetch_timeout', line: lineOf(text, 'fetch_timeout'), message: `"${s.fetchTimeout}" is not a duration. Use something like 5s.` });
    } else if (seconds(s.fetchTimeout) > 60) {
      issues.push({ severity: 'error', path: 'runner.fetch_timeout', line: lineOf(text, 'fetch_timeout'), message: 'The fetch timeout is at most 60s.' });
    }
  }
  for (const l of s.labels) {
    if (!/^[^:\s]+(:(host|docker:\/\/\S+))?$/.test(l)) {
      issues.push({ severity: 'error', path: 'runner.labels', line: lineOf(text, 'labels'), message: `The label "${l}" must look like name:host or name:docker://image.` });
    }
  }
  if (s.cacheSizeLimit !== null && !/^\d+\s*(B|KB|MB|GB|TB)?$/i.test(s.cacheSizeLimit)) {
    issues.push({ severity: 'error', path: 'cache.size_limit', line: lineOf(text, 'size_limit'), message: `"${s.cacheSizeLimit}" is not a size. Use something like 10GB, or 0 for no cap.` });
  }
  return issues;
}

/** What the form shows for a text. */
export function viewOf(text: string): ConfigView {
  const settings = readSettings(text);
  return {
    settings,
    unreadable: [],
    issues: checkText(text),
    containerRunner: settings.labels.some((l) => l.includes('docker://')),
  };
}

const CONTEXT = 3;

/** A line diff with `CONTEXT` lines around each change. */
export function diffText(a: string, b: string): TextDiff {
  const x = a === '' ? [] : a.split('\n');
  const y = b === '' ? [] : b.split('\n');
  const n = x.length;
  const m = y.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i]![j] = x[i] === y[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
  const all: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && x[i] === y[j]) {
      all.push({ kind: 'same', text: x[i]!, oldNo: i + 1, newNo: j + 1 });
      i++;
      j++;
    } else if (i < n && (j === m || lcs[i + 1]![j]! >= lcs[i]![j + 1]!)) {
      all.push({ kind: 'removed', text: x[i]!, oldNo: i + 1, newNo: null });
      i++;
    } else {
      all.push({ kind: 'added', text: y[j]!, oldNo: null, newNo: j + 1 });
      j++;
    }
  }
  const keep = new Array<boolean>(all.length).fill(false);
  all.forEach((l, k) => {
    if (l.kind === 'same') return;
    for (let c = Math.max(0, k - CONTEXT); c <= Math.min(all.length - 1, k + CONTEXT); c++) keep[c] = true;
  });
  const hunks: DiffHunk[] = [];
  let cur: DiffLine[] | null = null;
  all.forEach((l, k) => {
    if (!keep[k]) {
      cur = null;
      return;
    }
    if (!cur) {
      cur = [];
      hunks.push({ lines: cur });
    }
    cur.push(l);
  });
  return {
    hunks,
    added: all.filter((l) => l.kind === 'added').length,
    removed: all.filter((l) => l.kind === 'removed').length,
    truncated: false,
  };
}

/** The dotted keys whose values differ between two texts. */
export function changedKeysOf(a: string, b: string): string[] {
  const x = readSettings(a);
  const y = readSettings(b);
  const keys: string[] = [];
  for (const f of SCALARS) if (f.get(x) !== f.get(y)) keys.push(f.path);
  for (const f of BLOCKS) if (JSON.stringify(f.get(x)) !== JSON.stringify(f.get(y))) keys.push(f.path);
  return keys;
}

/** A short stable hash of the text (the engine uses SHA-256). */
export function fingerprint(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0') + text.length.toString(16).padStart(4, '0');
}

/** What saving `edit` over `current` would do. */
export function previewOf(current: string, edit: ConfigEdit): ConfigPreview {
  const text = edit.mode === 'raw' ? edit.text : applyForm(current, edit.settings);
  const changedKeys = changedKeysOf(current, text);
  return {
    text,
    changed: text !== current,
    restartNeeded: changedKeys.length > 0,
    diff: diffText(current, text),
    issues: checkText(text),
    changedKeys,
    labels: readSettings(text).labels.map((l) => l.split(':')[0] ?? l),
  };
}

import type { FieldSpec, Preset, Release, SourceKind, VersionSchemeId, WatchDraft } from '../api.types';
import { blankDraft } from '../draft';

export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;

/** A small deterministic generator so the demo looks the same on every start. */
export function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const text = (key: string, label: string, over: Partial<FieldSpec> = {}): FieldSpec => ({
  key,
  label,
  help: '',
  kind: 'text',
  required: true,
  default: '',
  placeholder: '',
  min: null,
  max: null,
  options: [],
  advanced: false,
  ...over,
});

/** The mock's catalog of source kinds (the real one comes from the engine). */
export const SOURCE_KINDS: SourceKind[] = [
  {
    id: 'folder',
    label: 'Folder or share',
    description: 'A local folder or a network share (UNC, SMB, NFS mount) with one entry per build.',
    group: 'files',
    fields: [
      text('path', 'Folder', { kind: 'folder', help: 'Each file or sub-folder name is a release.', placeholder: '/Volumes/Builds/acme' }),
      text('matchAgainst', 'Read the version from', {
        kind: 'choice',
        required: false,
        default: 'name',
        options: [
          { value: 'name', label: 'The file name' },
          { value: 'folder', label: 'The folder the file is in' },
          { value: 'path', label: 'The path below the folder' },
        ],
      }),
      text('depth', 'Folder levels', { kind: 'integer', required: false, default: '0', min: 0, max: 8, help: '0 looks only in this folder; higher numbers also look inside sub-folders.' }),
      text('extensions', 'File types', { required: false, placeholder: 'iso, exe, msi, zip', help: 'Optional: only these extensions, separated by commas.' }),
      text('skipHidden', 'Skip hidden files', { kind: 'toggle', required: false, default: 'false' }),
      text('maxEntries', 'Most entries per check', { kind: 'integer', required: false, default: '200000', min: 100, max: 5000000, advanced: true }),
      text('settleSeconds', 'Settle time (seconds)', { kind: 'integer', required: false, default: '5', min: 0, max: 300, advanced: true }),
    ],
  },
  {
    id: 'http-dir',
    label: 'Web directory',
    description: 'An HTTP index page that lists files, like a mirror or download server.',
    group: 'web',
    fields: [text('url', 'Directory URL', { kind: 'url', placeholder: 'https://downloads.example.org/tool/' })],
  },
  {
    id: 'feed',
    label: 'RSS or Atom feed',
    description: 'A release or blog feed. Paste a page address to find its feeds.',
    group: 'web',
    fields: [text('url', 'Feed URL', { kind: 'url', placeholder: 'https://example.org/releases.atom' })],
  },
  {
    id: 'json-api',
    label: 'JSON API',
    description: 'Any JSON endpoint that lists versions.',
    group: 'web',
    fields: [
      text('url', 'Endpoint', { kind: 'url', placeholder: 'https://api.example.org/versions' }),
      text('path', 'Where the versions are', { help: 'A path such as releases[].tag', placeholder: 'releases[].tag' }),
      text('token', 'Access token', { kind: 'secret', required: false, help: 'Kept in the credential store, never in the config.' }),
    ],
  },
  {
    id: 'web-page',
    label: 'Web page',
    description: 'A page that shows the latest version in its text. JavaScript pages are rendered when needed.',
    group: 'web',
    fields: [
      text('url', 'Page URL', { kind: 'url', placeholder: 'https://example.org/download' }),
      text('selector', 'Element to read', { required: false, help: 'A CSS selector; empty reads the whole page.' }),
      text('render', 'Page needs JavaScript', {
        kind: 'choice',
        required: false,
        default: 'auto',
        options: [
          { value: 'auto', label: 'Detect automatically' },
          { value: 'always', label: 'Always render' },
          { value: 'never', label: 'Never render' },
        ],
      }),
    ],
  },
  {
    id: 'github',
    label: 'GitHub',
    description: 'Releases of a GitHub repository.',
    group: 'forge',
    fields: [
      text('repo', 'Repository', { placeholder: 'owner/name', help: 'owner/name, or the repository URL.' }),
      text('token', 'Access token', { kind: 'secret', required: false, help: 'Only for private repositories or higher rate limits.' }),
    ],
  },
  {
    id: 'gitea',
    label: 'Gitea or Forgejo',
    description: 'Releases of a repository on a Gitea, Forgejo or Codeberg server.',
    group: 'forge',
    fields: [
      text('server', 'Server', { kind: 'url', default: 'https://codeberg.org' }),
      text('repo', 'Repository', { placeholder: 'owner/name' }),
      text('token', 'Access token', { kind: 'secret', required: false }),
    ],
  },
  {
    id: 'gitlab',
    label: 'GitLab',
    description: 'Releases of a GitLab project.',
    group: 'forge',
    fields: [
      text('server', 'Server', { kind: 'url', default: 'https://gitlab.com' }),
      text('project', 'Project', { placeholder: 'group/project' }),
      text('token', 'Access token', { kind: 'secret', required: false }),
    ],
  },
  {
    id: 'oci',
    label: 'Container image',
    description: 'Tags of an image in an OCI registry (Docker Hub, GHCR, a private registry).',
    group: 'registry',
    fields: [
      text('image', 'Image', { placeholder: 'ghcr.io/owner/name' }),
      text('token', 'Registry token', { kind: 'secret', required: false }),
    ],
  },
  {
    id: 'npm',
    label: 'npm package',
    description: 'Versions of a package on npm.',
    group: 'registry',
    fields: [text('package', 'Package', { placeholder: '@scope/name' })],
  },
  {
    id: 'crates',
    label: 'crates.io',
    description: 'Versions of a Rust crate.',
    group: 'registry',
    fields: [
      text('crate', 'Crate', { placeholder: 'serde' }),
      text('days', 'Ignore yanked after (days)', { kind: 'integer', required: false, default: '0', min: 0, max: 365 }),
    ],
  },
];

const preset = (id: string, name: string, description: string, category: string, kind: string, values: Record<string, string>, extra: Partial<WatchDraft> = {}): Preset => ({
  id,
  name,
  description,
  category,
  draft: blankDraft({ name, source: { kind, values }, ...extra }),
});

export const PRESETS: Preset[] = [
  preset('node-lts', 'Node.js', 'The JavaScript runtime. Stable releases only.', 'Runtimes', 'json-api', { url: 'https://nodejs.org/dist/index.json', path: '[].version' }),
  preset('rust', 'Rust toolchain', 'Stable Rust releases.', 'Runtimes', 'github', { repo: 'rust-lang/rust' }),
  preset('python', 'Python', 'CPython releases.', 'Runtimes', 'feed', { url: 'https://www.python.org/jobs/feed/rss/' }),
  preset('vscode', 'Visual Studio Code', 'The code editor, from its release notes feed.', 'Editors', 'github', { repo: 'microsoft/vscode' }),
  preset('git', 'Git', 'Git for every platform.', 'Developer tools', 'github', { repo: 'git/git' }),
  preset('postgres', 'PostgreSQL', 'Major and minor server releases.', 'Databases', 'feed', { url: 'https://www.postgresql.org/versions.rss' }),
  preset('nginx', 'nginx', 'Mainline releases.', 'Servers', 'http-dir', { url: 'https://nginx.org/download/' }),
  preset('7zip', '7-Zip', 'The archiver, from its download page.', 'Utilities', 'web-page', { url: 'https://www.7-zip.org/download.html', selector: 'b' }, { versions: 'numeric' }),
  preset('go', 'Go', 'The Go toolchain, stable releases.', 'Runtimes', 'json-api', { url: 'https://go.dev/dl/?mode=json', path: '[].version' }),
  preset('deno', 'Deno', 'The Deno runtime.', 'Runtimes', 'github', { repo: 'denoland/deno' }),
  preset('neovim', 'Neovim', 'The modal editor, stable releases.', 'Editors', 'github', { repo: 'neovim/neovim' }),
  preset('ripgrep', 'ripgrep', 'Fast recursive search.', 'Developer tools', 'github', { repo: 'BurntSushi/ripgrep' }),
  preset('grafana', 'Grafana', 'Dashboards and alerting.', 'Servers', 'github', { repo: 'grafana/grafana' }),
  preset('caddy', 'Caddy', 'The web server with automatic HTTPS.', 'Servers', 'github', { repo: 'caddyserver/caddy' }),
  preset('sqlite', 'SQLite', 'The embedded database, from its download page.', 'Databases', 'web-page', { url: 'https://www.sqlite.org/download.html', selector: 'td' }, { versions: 'numeric' }),
];

const SCHEME_FORMATS: Record<VersionSchemeId, (major: number, minor: number, patch: number, r: () => number) => string> = {
  semver: (a, b, c) => `${a}.${b}.${c}`,
  numeric: (a, b, c) => `${a}.${String(b).padStart(2, '0')}.${String(c).padStart(2, '0')}`,
  'build-number': (a, b, c, r) => `R${a}.${String(b).padStart(2, '0')}.${String(c).padStart(2, '0')}T${Math.floor(r() * 40) + 1}`,
  date: (a, b, c) => `${2024 + (a % 3)}-${String((b % 12) + 1).padStart(2, '0')}-${String((c % 28) + 1).padStart(2, '0')}`,
  custom: (a, b, c) => `${a}.${b}.${c}`,
};

/** `count` releases, oldest first, spread over `spanDays` ending at `end`. */
export function makeReleases(
  seed: number,
  scheme: VersionSchemeId,
  count: number,
  end: number,
  spanDays: number,
  namePrefix: string,
  base: [number, number, number] = [1, 0, 0],
): Release[] {
  const r = rng(seed);
  const out: Release[] = [];
  let [major, minor, patch] = base;
  const gaps: number[] = [];
  for (let i = 0; i < count; i++) gaps.push(0.3 + r());
  const total = gaps.reduce((a, b) => a + b, 0);
  let t = end - spanDays * DAY;
  for (let i = 0; i < count; i++) {
    t += (gaps[i]! / total) * spanDays * DAY;
    const roll = r();
    if (roll < 0.08) {
      major++;
      minor = 0;
      patch = 0;
    } else if (roll < 0.4) {
      minor++;
      patch = 0;
    } else patch++;
    const pre = r() < 0.12;
    const version = SCHEME_FORMATS[scheme](major, minor, patch, r) + (pre && scheme === 'semver' ? '-rc.1' : '');
    const at = Math.round(t);
    out.push({
      version,
      prerelease: pre,
      name: `${namePrefix}${version}`,
      location: `https://downloads.example.test/${namePrefix}${version}`,
      publishedAtMs: at,
      firstSeenMs: at + Math.round(r() * 3 * HOUR),
      size: Math.round(20_000_000 + r() * 180_000_000),
      sha256: Array.from({ length: 64 }, () => Math.floor(r() * 16).toString(16)).join(''),
      product: null,
      channel: '',
      fields: {},
      artifacts: [],
      updatedInPlace: false,
    });
  }
  return out;
}

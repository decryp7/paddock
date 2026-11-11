#!/usr/bin/env node
// The AppImage tools `tauri build --bundles appimage` would download from GitHub at bundle time (linuxdeploy and AppRun),
// pinned by URL and SHA-256 and kept in a cache folder, so a GitHub outage cannot fail a bundle (Dyno 1.1.1 WP-2: a 504 failed
// dyno-release 7693 and 7694 without an annotation). `tauri-bundle.sh` runs `install` before an AppImage build: each tool is
// verified (a cached copy too), downloaded only when missing or changed, and copied where Tauri's bundler looks for it
// (`$XDG_CACHE_HOME/tauri`, else `~/.cache/tauri`), which makes it skip its own download. The tauri-bundle action keeps the
// cache folder with actions/cache under the key `key` prints (the pins), so a warm cache needs no network at all.
//
//   node appimage-tools.mjs key                                  the actions/cache key of these pins
//   node appimage-tools.mjs install [--cache <dir>] [--tools <dir>]
//   node appimage-tools.mjs check [--tools <dir>]                a linuxdeploy we do not pin: a warning, an error on a tag
//
// The pins follow tauri-bundler 2.10.3's `prepare_tools` (src/bundle/linux/appimage/linuxdeploy.rs:208-278; @tauri-apps/cli
// 2.12.3 pins that bundler with `=2.10.3`): LINUXDEPLOY_COMMIT_HASH and the file names it expects. It downloads AppRun and
// linuxdeploy only when the file is missing (:212, :223) and zeroes linuxdeploy's AppImage magic with `dd` on every call
// (:267-276), so the copy this script puts there is patched by Tauri each build. A conformance test ties PINNED_FOR_CLI to the
// lockfile's @tauri-apps/cli; when the CLI moves, `check` also flags a linuxdeploy Tauri fetched itself.
//
// Integrity gap: linuxdeploy-plugin-appimage is NOT pinned and NOT verified. Tauri downloads it from linuxdeploy's moving
// `continuous` release whenever its tools folder lacks it (:248-263) and runs it unchecked; a failed download only falls back
// to the plugin built into linuxdeploy, so it cannot fail a bundle, but a successful one is unverified code in the build.
// Closing it (a mirrored build on Gitea's generic packages, pinned here) is a later plan row.
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/** linuxdeploy's release in tauri-apps/binary-releases (tauri-bundler's LINUXDEPLOY_COMMIT_HASH). */
export const LINUXDEPLOY = '07333c6';

/** The @tauri-apps/cli version the pins were read from (its tauri-bundler's prepare_tools); package-lock.json must match. */
export const PINNED_FOR_CLI = '2.12.3';

/**
 * The pinned tools per Tauri tools arch: the release URL, its SHA-256 (measured on download 2026-11-12 and equal to the digest
 * GitHub's release API lists for the asset) and the file name Tauri's bundler looks for in its tools folder.
 */
export const PINS = {
  x86_64: [
    {
      name: 'AppRun',
      version: 'apprun-old',
      url: 'https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64',
      sha256: 'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f',
      file: 'AppRun-x86_64',
    },
    {
      name: 'linuxdeploy',
      version: `linuxdeploy-${LINUXDEPLOY}`,
      url: `https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-${LINUXDEPLOY}/linuxdeploy-x86_64.AppImage`,
      sha256: '36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62',
      file: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`,
    },
  ],
};

/** Tauri's tools arch for a Node platform/arch, or null when nothing is pinned for it. */
export function toolsArch(platform = process.platform, arch = process.arch) {
  return platform === 'linux' && arch === 'x64' ? 'x86_64' : null;
}

/** The actions/cache key: each tool's version and the start of its SHA-256, so any pin change is a new key. */
export function cacheKey(arch = 'x86_64', pins = PINS[arch]) {
  if (!pins) throw new Error(`no pinned AppImage tools for ${arch}`);
  return ['appimage-tools', arch, ...pins.map((p) => `${p.version}-${p.sha256.slice(0, 12)}`)].join('-');
}

/** The folders: Tauri's tools folder as the `dirs` crate finds it on Linux, and our cache folder beside it. */
export function defaultDirs(env = process.env) {
  const base = env.XDG_CACHE_HOME && isAbsolute(env.XDG_CACHE_HOME) ? env.XDG_CACHE_HOME : join(env.HOME || homedir(), '.cache');
  return { cache: env.PADDOCK_APPIMAGE_TOOLS || join(base, 'paddock-appimage-tools'), tools: join(base, 'tauri') };
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** Whether a failed download is worth another try: no answer, a 5xx, 408 or 429; never another 4xx. */
export const retryable = (err) => err.status === null || err.status >= 500 || err.status === 408 || err.status === 429;

/** "answered HTTP 504" or "got no answer (ECONNREFUSED)". */
const outcome = (err) => (err.status === null ? `got no answer (${err.reason})` : `answered HTTP ${err.status}`);

/** A failed download: the URL, the HTTP status (null when no answer came) and the cause. */
export class DownloadError extends Error {
  constructor(url, status, cause) {
    super(`${url} ${status === null ? `got no answer (${cause})` : `answered HTTP ${status}`}`);
    this.url = url;
    this.status = status;
    this.reason = cause;
  }
}

/** GETs `url` into a Buffer; throws DownloadError on an HTTP error or no answer. */
export async function fetchBytes(url, { timeoutMs = 5 * 60 * 1000 } = {}) {
  let res;
  try {
    res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs), redirect: 'follow' });
  } catch (e) {
    throw new DownloadError(url, null, e.cause?.code ?? e.cause?.message ?? e.message);
  }
  if (!res.ok) throw new DownloadError(url, res.status);
  return Buffer.from(await res.arrayBuffer());
}

/** The one-line `::error::` for a failed download (ER-27): the tool, the URL, the status and the next step. */
export function downloadAnnotation(pin, err, key) {
  return `::error::Downloading ${pin.name} (${pin.version}) for the AppImage failed: ${pin.url} ${outcome(err)}. ` +
    `Next step: re-run the job when github.com answers again; a run that restores the actions/cache key ${key} needs no download.`;
}

/**
 * Makes each pinned tool of `arch` present and verified in `cache` (a cached copy whose SHA-256 differs is downloaded again),
 * then copies it into `tools` under the name Tauri expects, executable. Downloads try `attempts` times, `delayMs` apart and
 * doubling (`sleep` is injectable), and only for a retryable failure. Returns a list of { name, source: 'cache' | 'download' }.
 * Throws with `annotation` set on any failure; no `.part` file is left behind.
 */
export async function install({
  cache, tools, arch = 'x86_64', pins = PINS[arch], fetch: get = fetchBytes, attempts = 3, delayMs = 2000, sleep = wait, write = writeFileSync, log = console.log,
}) {
  const key = cacheKey(arch, pins);
  mkdirSync(cache, { recursive: true });
  mkdirSync(tools, { recursive: true });
  const done = [];
  for (const pin of pins) {
    const kept = join(cache, pin.file);
    let source = 'cache';
    if (existsSync(kept) && sha256(readFileSync(kept)) !== pin.sha256) {
      log(`::warning::The cached ${pin.name} (${kept}) does not match its pinned SHA-256: downloading it again.`);
      rmSync(kept, { force: true });
    }
    if (!existsSync(kept)) {
      source = 'download';
      let bytes;
      for (let i = 1; ; i++) {
        try {
          bytes = await get(pin.url);
          break;
        } catch (e) {
          const err = e instanceof DownloadError ? e : new DownloadError(pin.url, null, e.message);
          if (i >= attempts || !retryable(err)) throw Object.assign(new Error(err.message), { annotation: downloadAnnotation(pin, err, key) });
          log(`appimage-tools: ${err.message}; try ${i + 1} of ${attempts} in ${(delayMs * 2 ** (i - 1)) / 1000} s`);
          await sleep(delayMs * 2 ** (i - 1));
        }
      }
      const got = sha256(bytes);
      if (got !== pin.sha256) {
        throw Object.assign(new Error(`checksum mismatch for ${pin.url}`), {
          annotation: `::error::${pin.name} from ${pin.url} has SHA-256 ${got}, but ${pin.sha256} is pinned: the file was not used. ` +
            'Next step: a changed release asset is a supply-chain question, not a retry; compare it with the release page and ' +
            'move the pin in conformance/ci/appimage-tools.mjs only after checking why it changed.',
        });
      }
      const part = `${kept}.part`;
      try {
        write(part, bytes);
        renameSync(part, kept);
      } catch (e) {
        rmSync(part, { force: true });
        throw Object.assign(e, { annotation: `::error::Writing ${pin.name} to the cache ${cache} failed: ${e.message}. Next step: check the runner's free disk and the folder's rights.` });
      }
    }
    // Always copy over: Tauri zeroes linuxdeploy's AppImage magic in place on every build (prepare_tools, :267-276), so the
    // tools folder's copy is never the pinned bytes after a build; the verified copy in the cache is, and Tauri patches the
    // fresh copy again before it runs it.
    const dest = join(tools, pin.file);
    copyFileSync(kept, dest);
    chmodSync(dest, 0o755);
    done.push({ name: pin.name, source });
    log(`appimage-tools: ${pin.name} ${pin.version} (sha256 ${pin.sha256.slice(0, 12)}) from the ${source} -> ${dest}`);
  }
  return done;
}

/** linuxdeploy builds in `tools` that are not the pinned one: Tauri downloaded them itself, unpinned. */
export function unpinned(tools, pins = PINS.x86_64) {
  if (!existsSync(tools)) return [];
  const ours = new Set(pins.map((p) => p.file));
  return readdirSync(tools).filter((f) => /^linuxdeploy-[0-9a-f]+-.+\.AppImage$/.test(f) && !ours.has(f));
}

function option(argv, name) {
  const i = argv.indexOf(name);
  if (i < 0) return undefined;
  if (argv[i + 1] === undefined) throw new Error(`${name} needs a value`);
  return argv[i + 1];
}

/**
 * The command line; returns the exit code. `PADDOCK_APPIMAGE_PINS` names a JSON file { arch, pins, retryDelayMs? } that
 * replaces the pins and the host's arch, for the tests only: under `GITHUB_ACTIONS=true` it is refused (exit 2), so a CI run
 * always uses the pins above. `check` fails (exit 1) on a tag run (`GITHUB_REF` refs/tags/...), where a release must not ship
 * an unpinned linuxdeploy.
 */
export async function main(argv, env = process.env, deps = {}) {
  const [cmd, ...rest] = argv;
  for (let i = 0; i < rest.length; i += 2) {
    if (!['--cache', '--tools'].includes(rest[i])) throw new Error(`unknown option ${rest[i]}`);
  }
  const dirs = defaultDirs(env);
  const cache = option(rest, '--cache') ?? dirs.cache;
  const tools = option(rest, '--tools') ?? dirs.tools;
  const log = deps.log ?? console.log;
  if (env.PADDOCK_APPIMAGE_PINS && env.GITHUB_ACTIONS === 'true') {
    log('::error::PADDOCK_APPIMAGE_PINS is set in a CI run: it replaces the pinned AppImage tools and is for the tests only. Next step: remove it from the workflow or the runner environment.');
    return 2;
  }
  const override = env.PADDOCK_APPIMAGE_PINS ? JSON.parse(readFileSync(env.PADDOCK_APPIMAGE_PINS, 'utf8')) : null;
  const arch = override ? override.arch : deps.arch === undefined ? toolsArch() : deps.arch;
  const pins = override ? override.pins : PINS[arch];
  if (cmd === 'key') {
    log(cacheKey(arch ?? 'x86_64', pins ?? PINS.x86_64));
    return 0;
  }
  if (!arch || !pins) {
    log(`::warning::No pinned AppImage tools for ${process.platform}/${process.arch}: Tauri downloads linuxdeploy and AppRun itself.`);
    return 0;
  }
  if (cmd === 'install') {
    try {
      await install({ cache, tools, arch, pins, log, ...(deps.fetch ? { fetch: deps.fetch } : {}), ...(override?.retryDelayMs !== undefined ? { delayMs: override.retryDelayMs } : {}), ...(deps.delayMs !== undefined ? { delayMs: deps.delayMs } : {}), ...(deps.sleep ? { sleep: deps.sleep } : {}), ...(deps.write ? { write: deps.write } : {}) });
      return 0;
    } catch (e) {
      log(e.annotation ?? `::error::Preparing the AppImage tools failed: ${e.message}. Next step: see conformance/ci/appimage-tools.mjs.`);
      return 1;
    }
  }
  if (cmd === 'check') {
    const tag = (env.GITHUB_REF ?? '').startsWith('refs/tags/');
    const found = unpinned(tools, pins);
    for (const f of found) {
      log(`::${tag ? 'error' : 'warning'}::Tauri downloaded ${f} itself (unpinned, from GitHub): its linuxdeploy is no longer ${LINUXDEPLOY}. ` +
        'Next step: pin the new build in conformance/ci/appimage-tools.mjs (URL and SHA-256 from tauri-bundler\'s prepare_tools).');
    }
    return tag && found.length ? 1 : 0;
  }
  throw new Error('usage: appimage-tools.mjs key | install [--cache <dir>] [--tools <dir>] | check [--tools <dir>]');
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
      console.log(`::error::appimage-tools: ${e.message}`);
      process.exit(2);
    },
  );
}

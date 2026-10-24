// Where an app's version is written, and how to read it from each place.
export const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function jsonVersion(text) {
  try {
    const v = JSON.parse(text).version;
    return typeof v === 'string' && SEMVER.test(v) ? v : null;
  } catch {
    return null;
  }
}

/** First `version = "x"` of a TOML table `[header]`. */
function tomlVersion(text, header) {
  const at = text.indexOf(`[${header}]`);
  if (at < 0) return null;
  const rest = text.slice(at + header.length + 2);
  const end = rest.search(/^\[/m);
  const m = /^version\s*=\s*"([^"]+)"/m.exec(end < 0 ? rest : rest.slice(0, end));
  return m && SEMVER.test(m[1]) ? m[1] : null;
}

/**
 * Every place the version is written: [{ file, version }]. A placeholder 0.0.0 in package.json is not a version. A tauri.conf.json
 * `version` that is a path (it reads package.json) or Cargo's `version.workspace = true` defer to their source.
 */
export function versionSources(ctx) {
  const dir = ctx.config.tauriDir;
  const out = [];
  const add = (file, version) => version && out.push({ file, version });
  add(`${dir}/tauri.conf.json`, jsonVersion(ctx.read(`${dir}/tauri.conf.json`) ?? ''));
  const pkg = jsonVersion(ctx.read(`${ctx.config.appDir}/package.json`) ?? '');
  if (pkg && pkg !== '0.0.0') add(`${ctx.config.appDir}/package.json`, pkg);
  const cargo = ctx.read(`${dir}/Cargo.toml`) ?? '';
  add(`${dir}/Cargo.toml`, tomlVersion(cargo, 'package'));
  if (/^version\.workspace\s*=\s*true/m.test(cargo)) add('Cargo.toml', tomlVersion(ctx.read('Cargo.toml') ?? '', 'workspace.package'));
  const log = ctx.read('CHANGELOG.md') ?? '';
  const head = /^##\s+\[?v?(\d+\.\d+\.\d+[^\]\s]*)\]?/m.exec(log);
  if (head && SEMVER.test(head[1])) add('CHANGELOG.md', head[1]);
  return out;
}

/** The app version: tauri.conf.json first, then the other sources. Null when none is readable. */
export function appVersion(ctx) {
  return versionSources(ctx)[0]?.version ?? null;
}

import { readFileSync } from 'node:fs';

/**
 * The app's version, read from its single source (rule 6: tests never hard-code it): a `package.json` (`version`) or a
 * `Cargo.toml` (`version = "…"` of its `[package]`). Throws unless it has a release-number shape.
 */
export const RELEASE_VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

export function readVersion(path: string): string {
  const text = readFileSync(path, 'utf8');
  const version = path.endsWith('.toml')
    ? /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(text)?.[1]
    : (JSON.parse(text) as { version?: unknown }).version;
  if (typeof version !== 'string' || !RELEASE_VERSION.test(version)) throw new Error(`${path}: no release-number version (found ${JSON.stringify(version)})`);
  return version;
}

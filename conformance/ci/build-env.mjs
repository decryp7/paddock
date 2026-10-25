#!/usr/bin/env node
// The build's commit and date for the app's About page (plan X10 gap: paddock-updater's About reads them from
// `option_env!`, but no CI set them). Writes to $GITHUB_ENV (or stdout as NAME=value lines):
//
//   PADDOCK_BUILD_COMMIT   the short commit (12 hex digits, or unknown)
//   PADDOCK_BUILD_DATE     the UTC date, YYYY-MM-DD
//
// The app reads them with `.build_info(option_env!("PADDOCK_BUILD_COMMIT"), option_env!("PADDOCK_BUILD_DATE"))` and prints
// `cargo:rerun-if-env-changed=` for both in build.rs. The commit comes from GITHUB_SHA, else `git rev-parse`; never fails.
import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** { PADDOCK_BUILD_COMMIT, PADDOCK_BUILD_DATE } from the environment; `head` and `now` are injectable. */
export function buildEnv({ env = process.env, head = gitHead, now = () => new Date() } = {}) {
  const sha = /^[0-9a-f]{7,64}$/i.test(env.GITHUB_SHA ?? '') ? env.GITHUB_SHA : head();
  const date = now().toISOString().slice(0, 10);
  return { PADDOCK_BUILD_COMMIT: sha && /^[0-9a-f]{7,64}$/i.test(sha) ? sha.toLowerCase().slice(0, 12) : 'unknown', PADDOCK_BUILD_DATE: date };
}

function gitHead() {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { stdio: ['ignore', 'pipe', 'ignore'], timeout: 5000 }).toString().trim();
  } catch {
    return null;
  }
}

if (isMain(import.meta.url)) {
  const lines = Object.entries(buildEnv()).map(([k, v]) => `${k}=${v}`);
  if (process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV, `${lines.join('\n')}\n`);
  console.log(lines.join('\n'));
}

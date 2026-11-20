// What "this CI run is a tag run" means, defined once: the ref is a tag (`refs/tags/...`, GITHUB_REF or GITEA_REF) or a ref type says so
// (GITHUB_REF_TYPE or GITEA_REF_TYPE, each on its own). JavaScript callers import `isTagRun`. Shell callers run `node is-tag-run.mjs`:
// exit 0 is a tag run, 3 is not; ANY other status (a crash, a missing file, a bad import) is a failure to tell, which they must treat as
// an error and never as "not a tag" (updater-key.sh's `updater_key_tag_run` and apps/dyno/scripts/check-bundle.sh do).
import { isMain } from './is-main.mjs';

export const NOT_A_TAG = 3;

/** True on a tag run, from the CI environment `env`. */
export function isTagRun(env = process.env) {
  return [env.GITHUB_REF, env.GITEA_REF].some((r) => /^refs\/tags\//.test(r ?? '')) || [env.GITHUB_REF_TYPE, env.GITEA_REF_TYPE].some((t) => t === 'tag');
}

if (isMain(import.meta.url)) process.exit(isTagRun() ? 0 : NOT_A_TAG);

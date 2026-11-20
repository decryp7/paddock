// What "this CI run is a tag run" means, defined once: the ref is a tag (`refs/tags/...`, GITHUB_REF or GITEA_REF) or a ref type says so
// (GITHUB_REF_TYPE or GITEA_REF_TYPE, each on its own). JavaScript callers import `isTagRun`. Shell callers run `node is-tag-run.mjs`:
// exit 0 is a tag run, 3 is not; ANY other status (a crash, a missing file, a bad import) is a failure to tell, which they must treat as
// an error and never as "not a tag" (updater-key.sh's `updater_key_tag_run` and apps/dyno/scripts/check-bundle.sh do).
//
// The tag rehearsal (.gitea/workflows/tag-rehearsal.yml) runs a tag's release path on a branch: PADDOCK_TAG_REHEARSAL=1 makes the answer
// "tag" for every release-path decision, and the callers then use a throw-away signing key and never publish (updater-key.sh,
// app-release.yml). It can never be combined with a real tag: a tag ref with the variable set is REFUSED (exit 4, a throw in JavaScript),
// and so is any value other than 1, 0 or empty, so the one run that could reach the real key and the rehearsal switch never coexist.
import { isMain } from './is-main.mjs';

export const NOT_A_TAG = 3;
export const REFUSED = 4;
export const REHEARSAL_VAR = 'PADDOCK_TAG_REHEARSAL';

/** True when the CI environment says the ref is a real tag (the rehearsal switch is not looked at). */
export function isRealTagRef(env = process.env) {
  return [env.GITHUB_REF, env.GITEA_REF].some((r) => /^refs\/tags\//.test(r ?? '')) || [env.GITHUB_REF_TYPE, env.GITEA_REF_TYPE].some((t) => t === 'tag');
}

/** True when this is a tag rehearsal (PADDOCK_TAG_REHEARSAL=1); throws on a value that is not 1, 0 or empty (fail closed). */
export function isRehearsal(env = process.env) {
  const v = env[REHEARSAL_VAR] ?? '';
  if (v === '' || v === '0') return false;
  if (v === '1') return true;
  throw new Error(`${REHEARSAL_VAR}=${JSON.stringify(v)} is not 1, 0 or empty: refused`);
}

/** True on a tag run (or a tag rehearsal), from the CI environment `env`. Throws when a rehearsal is switched on in a run that is a real tag. */
export function isTagRun(env = process.env) {
  if (isRehearsal(env)) {
    if (isRealTagRef(env)) throw new Error(`${REHEARSAL_VAR}=1 on a real tag run (GITHUB_REF=${env.GITHUB_REF ?? env.GITEA_REF ?? 'unset'}): a rehearsal never runs on a tag, so the real signing key and the rehearsal switch can never meet`);
    return true;
  }
  return isRealTagRef(env);
}

if (isMain(import.meta.url)) {
  try {
    process.exit(isTagRun() ? 0 : NOT_A_TAG);
  } catch (e) {
    console.error(`::error::is-tag-run: ${e.message}. Next step: run the rehearsal from the default branch only (tag-rehearsal.yml), never from a tag, and set ${REHEARSAL_VAR} to 1 only there.`);
    process.exit(REFUSED);
  }
}

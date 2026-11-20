// What "this CI run is a tag run" means, defined once: the ref is a tag (`refs/tags/...`, GITHUB_REF or GITEA_REF) or the ref type says so.
// JavaScript callers import `isTagRun`; shell callers run `node is-tag-run.mjs` (exit 0 on a tag run, 1 otherwise): updater-key.sh's
// `updater_key_on_tag` and apps/dyno/scripts/check-bundle.sh do.
import { isMain } from './is-main.mjs';

/** True on a tag run, from the CI environment `env`. */
export function isTagRun(env = process.env) {
  return /^refs\/tags\//.test(`${env.GITHUB_REF ?? ''}`) || /^refs\/tags\//.test(`${env.GITEA_REF ?? ''}`) || `${env.GITHUB_REF_TYPE ?? ''}${env.GITEA_REF_TYPE ?? ''}` === 'tag';
}

if (isMain(import.meta.url)) process.exit(isTagRun() ? 0 : 1);

// C56: every app that shows the build's commit and date reads the names the build-env action sets (ER-21). Dyno 1.0 called
// `.build_info(option_env!("DYNO_COMMIT"), option_env!("DYNO_BUILT"))` while `.gitea/actions/build-env` sets
// PADDOCK_BUILD_COMMIT and PADDOCK_BUILD_DATE, so its About page showed no commit and no date. The names are read from the
// action itself; an app's `.build_info(` call (any Rust file under apps/) must pass `option_env!` of exactly those two, in
// the order commit, date.
import { error } from '../finding.mjs';

const ACTION = '.gitea/actions/build-env/action.yml';
const code = (text) => text.replace(/\/\/.*$/gm, '');

/** The build-env names the action sets: the `PADDOCK_BUILD_*` words of its description, in order (commit, then date). */
export function actionNames(yml) {
  return [...new Set(yml.match(/\bPADDOCK_BUILD_[A-Z]+\b/g) ?? [])];
}

/** What is wrong with each `.build_info(` call in `src`: the `option_env!` names it reads differ from `names`. */
export function buildInfoProblems(src, names) {
  const out = [];
  const text = code(src);
  for (const m of text.matchAll(/\.build_info\s*\(/g)) {
    // The call's arguments: up to the parenthesis that closes the call.
    let depth = 1;
    let end = m.index + m[0].length;
    for (; end < text.length && depth > 0; end++) depth += text[end] === '(' ? 1 : text[end] === ')' ? -1 : 0;
    const args = text.slice(m.index + m[0].length, end);
    const read = [...args.matchAll(/option_env!\s*\(\s*"([^"]*)"\s*\)/g)].map((x) => x[1]);
    if (read.length !== names.length || read.some((n, i) => n !== names[i])) {
      out.push(`.build_info reads ${read.length ? read.join(' and ') : 'no option_env! names'}, but build-env sets ${names.join(' and ')}: the About page shows no commit or date`);
    }
  }
  return out;
}

export default {
  id: 'C56',
  title: 'Every app reads the build commit and date under the names build-env sets',
  rule: 43,
  run(ctx) {
    const callers = ctx.files().filter((f) => f.startsWith('apps/') && f.endsWith('.rs') && /\.build_info\s*\(/.test(code(ctx.read(f) ?? '')));
    if (!callers.length) return [];
    const names = actionNames(ctx.read(ACTION) ?? '');
    if (names.length !== 2) {
      return [error('C56', 43, ACTION, 1, 'cannot read the two PADDOCK_BUILD_* names from the build-env action', 'Name PADDOCK_BUILD_COMMIT and PADDOCK_BUILD_DATE in the action description.')];
    }
    return callers.flatMap((f) =>
      buildInfoProblems(ctx.read(f) ?? '', names).map((message) =>
        error('C56', 43, f, 1, message, `Use .build_info(option_env!("${names[0]}"), option_env!("${names[1]}")) (and rerun-if-env-changed for both in build.rs).`),
      ),
    );
  },
};

// C111 (rule 3: fix everything locally first; ER-9): every workflow and every composite action parses as YAML. The other
// workflow checks read the files with line scanners (util/workflows.mjs), which accept text a real parser rejects, and a
// Gitea run that cannot parse a file fails (or silently drops a value) only after the push. The trigger, 2026-11-12: an
// unquoted `if: !cancelled()` is a YAML tag, not the expression, and the `yaml` package reports it as the warning "Unresolved
// tag"; it is an error here. One finding per problem, with file, line, message and the next step.
//
// Files: `.gitea/workflows/*.yml|yaml` and `.gitea/actions/**/action.yml|yaml`.
// Positive controls (ER-33): no file found, or the `yaml` package not installed, is "check broken" (an error no waiver covers),
// never a pass. The package is loaded here, not at the top, so a run of another check needs no `npm ci` in conformance/.
import { error, unwaivable } from '../finding.mjs';

const FILE = /^\.gitea\/(workflows\/[^/]+|actions\/(?:.+\/)?action)\.ya?ml$/;

const NEXT = 'Fix the YAML at this line. A value that starts with `!`, `*`, `&`, `@` or a backtick must be quoted, for example `if: ${{ !cancelled() }}`. Re-run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C111';

/** The parse problems of one YAML text: [{ line, message }], errors and warnings alike (a warning is a value read wrongly). */
export function yamlProblems(text, parseDocument) {
  const doc = parseDocument(text, { prettyErrors: true, uniqueKeys: true });
  return [...doc.errors, ...doc.warnings].map((e) => ({ line: e.linePos?.[0]?.line ?? 1, message: String(e.message).split('\n')[0].replace(/ at line \d+, column \d+:?$/, '') }));
}

export default {
  id: 'C111',
  title: 'workflows and actions parse as YAML',
  rule: 3,
  scope: 'root',
  mode: 'gate',
  async run(ctx) {
    let parseDocument;
    try {
      ({ parseDocument } = await import('yaml'));
    } catch (e) {
      return [unwaivable(error('C111', 3, 'conformance/package.json', 1, `check broken: the yaml package cannot be loaded (${e.code ?? e.message})`, 'Run `npm ci --ignore-scripts` in conformance/ (its package-lock.json pins yaml 2).'))];
    }
    const files = ctx.files().filter((f) => FILE.test(f));
    if (files.length === 0) {
      return [unwaivable(error('C111', 3, '.gitea', 1, 'check broken: no workflow or action file was found to parse', 'Run it with --root at the repository root; a tree without .gitea/ should not select C111.'))];
    }
    const out = [];
    for (const file of files) {
      const text = ctx.read(file);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C111', 3, file, 1, 'check broken: the file cannot be read', 'Fix the file permissions or remove the file.')));
        continue;
      }
      for (const { line, message } of yamlProblems(text, parseDocument)) {
        out.push(error('C111', 3, file, line, `YAML: ${message}`, NEXT));
      }
    }
    return out;
  },
};

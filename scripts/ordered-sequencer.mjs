// A Vitest sequencer that runs spec files in exactly the order given in the environment (rule 58, ER-29).
//
// Vitest's default sequencer (BaseSequencer) runs the previously failed files first and sorts the rest by cached duration,
// `--sequence.shuffle` then reorders each subset, and workers split the files, so "the files in the order I passed them"
// is not what runs. This sequencer returns the files in the order of the PADDOCK_TEST_ORDER environment variable (absolute
// paths, one per line); a file that is not listed runs after the listed ones in its original order. It does not shard.
//
// Vitest accepts a sequencer only as a class in the config (the CLI option `--sequence.sequencer=<path>` is read as a string
// and fails with "Sequencer is not a constructor"), so a project opts in with one line in its Vitest config:
//
//   import { orderedSequence } from '<path to>/scripts/ordered-sequencer.mjs';
//   export default defineConfig({ test: { sequence: { ...orderedSequence() } } });
//
// `orderedSequence()` returns {} unless PADDOCK_TEST_ORDER is set, so normal runs are unchanged. scripts/find-polluter.mjs sets
// PADDOCK_TEST_ORDER and `--no-file-parallelism --maxWorkers=1 --sequence.shuffle=false` for each run.
// Pure core: `orderFiles` has no Vitest dependency.

/** The absolute path of a Vitest test specification (`moduleId`), of a legacy `[project, file]` pair, or of a plain string. */
export function specPath(spec) {
  if (typeof spec === 'string') return spec;
  if (Array.isArray(spec)) return spec[1];
  return spec.moduleId ?? spec.file ?? String(spec);
}

/** Returns `files` ordered by `order` (absolute paths); unlisted files keep their relative order after the listed ones. */
export function orderFiles(files, order) {
  const rank = new Map(order.map((p, i) => [p, i]));
  const listed = files.filter((f) => rank.has(specPath(f))).sort((a, b) => rank.get(specPath(a)) - rank.get(specPath(b)));
  return [...listed, ...files.filter((f) => !rank.has(specPath(f)))];
}

/** Parses PADDOCK_TEST_ORDER: one path per line, blank lines ignored. */
export function parseOrder(value) {
  return (value ?? '')
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l !== '');
}

export default class OrderedSequencer {
  constructor(ctx) {
    this.ctx = ctx;
  }

  async shard(files) {
    return files;
  }

  async sort(files) {
    return orderFiles(files, parseOrder(process.env.PADDOCK_TEST_ORDER));
  }
}

/** The `test.sequence` overlay: the ordered sequencer and no shuffle when PADDOCK_TEST_ORDER is set, else nothing. */
export function orderedSequence(env = process.env) {
  return parseOrder(env.PADDOCK_TEST_ORDER).length > 0 ? { sequencer: OrderedSequencer, shuffle: false } : {};
}

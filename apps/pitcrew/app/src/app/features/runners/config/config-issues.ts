import type { ConfigIssue, DiffKind } from '../../../core/api.types';

/** Pure helpers over the engine's findings and diff. Nothing is validated here: the messages come from Rust. */

/** The form field a finding belongs to; `container` shows next to the container group, `` at the file level. */
export type IssuePath =
  | 'runner.capacity'
  | 'runner.timeout'
  | 'runner.fetch_interval'
  | 'runner.fetch_timeout'
  | 'runner.labels'
  | 'runner.envs'
  | 'cache.enabled'
  | 'cache.dir'
  | 'cache.size_limit'
  | 'container.network'
  | 'container.privileged'
  | 'container.options'
  | 'container.valid_volumes'
  | 'log.level'
  | 'container';

/** The findings about one key. A finding about a child (`runner.envs.FOO`) belongs to its parent field. */
export function issuesFor(issues: readonly ConfigIssue[], path: IssuePath): ConfigIssue[] {
  return issues.filter((i) => i.path === path || i.path.startsWith(`${path}.`));
}

/** Findings that no form field shows: the file as a whole, or keys the form does not know. */
export function otherIssues(issues: readonly ConfigIssue[], known: readonly IssuePath[]): ConfigIssue[] {
  return issues.filter((i) => !known.some((k) => i.path === k || i.path.startsWith(`${k}.`)));
}

export function errorCount(issues: readonly ConfigIssue[]): number {
  return issues.filter((i) => i.severity === 'error').length;
}

export function warningCount(issues: readonly ConfigIssue[]): number {
  return issues.length - errorCount(issues);
}

/** The status line under the Advanced editor. */
export function validity(issues: readonly ConfigIssue[]): { ok: boolean; text: string } {
  const e = errorCount(issues);
  const w = warningCount(issues);
  if (e > 0) return { ok: false, text: `${e} ${e === 1 ? 'error' : 'errors'}${w > 0 ? `, ${w} ${w === 1 ? 'warning' : 'warnings'}` : ''}` };
  return { ok: true, text: w > 0 ? `Valid YAML, ${w} ${w === 1 ? 'warning' : 'warnings'}` : 'Valid YAML' };
}

/** The symbol in front of a diff line, so added and removed lines differ without colour. */
export function diffSymbol(kind: DiffKind): string {
  return kind === 'added' ? '+' : kind === 'removed' ? '−' : ' ';
}

/** The screen-reader word for a diff line. */
export function diffWord(kind: DiffKind): string {
  return kind === 'added' ? 'Added' : kind === 'removed' ? 'Removed' : 'Unchanged';
}

import { appendFileSync, writeFileSync } from 'node:fs';

/*
 * A Playwright reporter that lists the tests that passed only after a retry (flaky), so a retry never hides a flake:
 * printed at the end, written to `playwright-retried.md` and appended to `$GITHUB_STEP_SUMMARY` when set. It never
 * changes the exit code (fixing the flake is a test task). Use it as `reporter: [['list'], ['@decryp7/paddock-e2e/retried-reporter']]`.
 */

/** One test that passed after a retry. */
export interface Flaky {
  readonly project: string;
  readonly file: string;
  readonly title: string;
  readonly retries: number;
}

/** Markdown for the flaky tests (empty when there are none). */
export function renderFlaky(flaky: readonly Flaky[]): string {
  if (!flaky.length) return '';
  const cell = (s: string) => s.replaceAll('|', '\\|');
  const rows = flaky.map((f) => `| ${cell(f.project)} | ${cell(f.file)} | ${cell(f.title)} | ${f.retries} |`).join('\n');
  return `### Flaky tests (passed only after a retry)\n\n| Project | File | Test | Retries |\n| --- | --- | --- | --- |\n${rows}\n`;
}

/** The parts of Playwright's `TestCase` the reporter reads. */
interface TestLike {
  readonly parent: { project(): { readonly name: string } | undefined };
  readonly location: { readonly file: string };
  titlePath(): string[];
}

export default class RetriedReporter {
  readonly flaky: Flaky[] = [];
  private readonly out: { readonly file?: string; readonly env?: Readonly<Record<string, string | undefined>> };

  /** `file`: where the Markdown goes (default `playwright-retried.md`); `env`: for tests. */
  constructor(out: { readonly file?: string; readonly env?: Readonly<Record<string, string | undefined>> } = {}) {
    this.out = out;
  }

  onTestEnd(test: TestLike, result: { readonly status: string; readonly retry: number }): void {
    if (result.status !== 'passed' || result.retry === 0) return;
    this.flaky.push({
      project: test.parent.project()?.name ?? '',
      file: test.location.file.replace(/^.*[\\/]e2e[\\/]/, ''),
      // The title path is ['', project, file, ...describe, test].
      title: test.titlePath().slice(3).join(' > '),
      retries: result.retry,
    });
  }

  onEnd(): void {
    const md = renderFlaky(this.flaky);
    if (!md) return;
    console.log(`\n${md}`);
    const env = this.out.env ?? process.env;
    try {
      writeFileSync(this.out.file ?? 'playwright-retried.md', md);
      if (env['GITHUB_STEP_SUMMARY']) appendFileSync(env['GITHUB_STEP_SUMMARY'], `${md}\n`);
    } catch {
      // The summary is a convenience; the console list above is the record.
    }
  }

  printsToStdio(): boolean {
    return false;
  }
}

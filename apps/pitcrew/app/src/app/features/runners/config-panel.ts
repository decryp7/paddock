import { Dialog } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, input, signal, untracked, viewChild } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { ConfigIssue, LogLevel, RunnerSettings } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatRelative } from '../../shared/format';
import { LoadState } from '../../shared/ui/load-state';
import { StatusStore } from '../../state/status.store';
import { reviewConfigChanges } from './config/config-diff-dialog';
import { type IssuePath, issuesFor, otherIssues, validity } from './config/config-issues';
import { FieldIssues } from './config/field-issues';
import { StringList } from './config/string-list';
import { MAX_BACKUPS } from './config/limits';
import { RunnerConfigStore, type ConfigMode } from './runner-config.store';

/** A text field of the form: its setting, the engine's key and the words around it. */
export interface TextField {
  key: 'timeout' | 'fetchInterval' | 'fetchTimeout' | 'cacheDir' | 'cacheSizeLimit' | 'containerNetwork' | 'containerOptions';
  path: IssuePath;
  label: string;
  placeholder: string;
  hint: string;
}

export const JOB_FIELDS: TextField[] = [
  { key: 'timeout', path: 'runner.timeout', label: 'Job timeout', placeholder: '3h', hint: 'Longest a job may run, such as 3h or 90m.' },
  { key: 'fetchInterval', path: 'runner.fetch_interval', label: 'Fetch interval', placeholder: '2s', hint: 'How often it asks the server for work.' },
  { key: 'fetchTimeout', path: 'runner.fetch_timeout', label: 'Fetch timeout', placeholder: '5s', hint: 'How long one such request may take (at most 60s).' },
];
export const CACHE_FIELDS: TextField[] = [
  { key: 'cacheDir', path: 'cache.dir', label: 'Cache folder', placeholder: 'the runner’s own folder', hint: 'Where cache data is stored.' },
  { key: 'cacheSizeLimit', path: 'cache.size_limit', label: 'Size limit', placeholder: 'no limit', hint: 'Cap for the whole cache, such as 10GB; 0 is no cap.' },
];
export const CONTAINER_FIELDS: TextField[] = [
  { key: 'containerNetwork', path: 'container.network', label: 'Network', placeholder: 'a new network per job', hint: 'The Docker network jobs join.' },
  { key: 'containerOptions', path: 'container.options', label: 'Extra options', placeholder: 'none', hint: 'Extra docker run options for every job container.' },
];

export const LOG_LEVELS: LogLevel[] = ['trace', 'debug', 'info', 'warn', 'error'];

const FORM_PATHS: IssuePath[] = [
  'runner.capacity',
  'runner.timeout',
  'runner.fetch_interval',
  'runner.fetch_timeout',
  'runner.labels',
  'runner.envs',
  'cache.enabled',
  'cache.dir',
  'cache.size_limit',
  'container.network',
  'container.privileged',
  'container.options',
  'container.valid_volumes',
  'log.level',
  'container',
];

/** Reads the capacity box: empty is unset, anything else a whole number from 0 (the engine says what is allowed). */
export function parseCapacity(value: string): number | null {
  if (value.trim() === '') return null;
  const n = Number(value);
  return Number.isFinite(n) ? Math.max(0, Math.trunc(n)) : null;
}

/** A tri-state select's value as the setting. */
export function parseTriState(value: string): boolean | null {
  return value === 'on' ? true : value === 'off' ? false : null;
}

export function triState(v: boolean | null): 'default' | 'on' | 'off' {
  return v === null ? 'default' : v ? 'on' : 'off';
}

/** The runner's `config.yaml`: a form for the common settings, an Advanced YAML editor, a review of the diff, and saving. */
@Component({
  selector: 'pc-config-panel',
  imports: [MatIcon, LoadState, FieldIssues, StringList],
  providers: [RunnerConfigStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './config-panel.html',
  styleUrl: './config-panel.scss',
})
export class ConfigPanel {
  readonly runner = input.required<string>();

  protected readonly store = inject(RunnerConfigStore);
  private readonly status = inject(StatusStore);
  private readonly clock = inject(ClockService);
  private readonly dialog = inject(Dialog);

  protected readonly jobFields = JOB_FIELDS;
  protected readonly cacheFields = CACHE_FIELDS;
  protected readonly containerFields = CONTAINER_FIELDS;
  protected readonly levels = LOG_LEVELS;
  protected readonly triState = triState;

  protected readonly newEnvName = signal('');
  protected readonly newEnvValue = signal('');
  private readonly editor = viewChild<ElementRef<HTMLTextAreaElement>>('editor');
  private readonly scroller = viewChild<ElementRef<HTMLElement>>('scroller');

  protected readonly snapshot = computed(() => this.status.runners().find((r) => r.name === this.runner()) ?? null);
  protected readonly envEntries = computed(() => Object.entries(this.store.settings()?.envs ?? {}));
  protected readonly rawLines = computed(() => this.store.rawText().split('\n'));
  protected readonly longest = computed(() => this.rawLines().reduce((m, l) => Math.max(m, l.length), 0));
  protected readonly issueLines = computed(() => new Set(this.store.issues().filter((i) => i.line !== null && i.severity === 'error').map((i) => i.line)));
  protected readonly warnLines = computed(() => new Set(this.store.issues().filter((i) => i.line !== null && i.severity === 'warning').map((i) => i.line)));
  protected readonly status2 = computed(() => validity(this.store.issues()));
  protected readonly extra = computed(() => otherIssues(this.store.issues(), FORM_PATHS));
  protected readonly restartPending = computed(() => (this.store.file()?.restartPending ?? false) || this.snapshot()?.pending === 'restart');
  protected readonly maxBackups = MAX_BACKUPS;

  constructor() {
    effect(() => {
      const name = this.runner();
      untracked(() => void this.store.load(name));
    });
    // A restart (the config being applied) writes a new label check to the history.
    effect(() => {
      const restarted = this.snapshot()?.runningSince ?? null;
      untracked(() => void this.store.refreshLabelCheck(restarted));
    });
  }

  protected forPath(path: IssuePath): ConfigIssue[] {
    return issuesFor(this.store.issues(), path);
  }

  protected hasIssue(path: IssuePath): boolean {
    return this.forPath(path).length > 0;
  }

  protected hasError(path: IssuePath): boolean {
    return this.forPath(path).some((i) => i.severity === 'error');
  }

  protected when(at: number): string {
    return formatRelative(at, this.clock.now());
  }

  protected set(change: Partial<RunnerSettings>): void {
    this.store.patch(change);
  }

  protected text(key: TextField['key'], value: string): void {
    this.store.patch({ [key]: value.trim() === '' ? null : value } as Partial<RunnerSettings>);
  }

  protected setCapacity(value: string): void {
    this.store.patch({ capacity: parseCapacity(value) });
  }

  protected setTri(key: 'cacheEnabled' | 'containerPrivileged', value: string): void {
    this.store.patch({ [key]: parseTriState(value) } as Partial<RunnerSettings>);
  }

  protected setLevel(value: string): void {
    this.store.patch({ logLevel: LOG_LEVELS.find((l) => l === value) ?? null });
  }

  protected setEnv(name: string, value: string): void {
    this.store.patch({ envs: { ...this.store.settings()?.envs, [name]: value } });
  }

  protected removeEnv(name: string): void {
    const { [name]: _gone, ...rest } = this.store.settings()?.envs ?? {};
    this.store.patch({ envs: rest });
  }

  protected addEnv(e?: Event): void {
    e?.preventDefault();
    const name = this.newEnvName().trim();
    if (name === '') return;
    this.setEnv(name, this.newEnvValue());
    this.newEnvName.set('');
    this.newEnvValue.set('');
  }

  protected mode(mode: ConfigMode): void {
    this.store.setMode(mode);
  }

  /** Puts the cursor on a line of the Advanced editor (a click on a finding). */
  protected jump(line: number | null): void {
    const ta = this.editor()?.nativeElement;
    if (line === null || !ta) return;
    const lines = this.rawLines();
    const offset = lines.slice(0, line - 1).reduce((n, l) => n + l.length + 1, 0);
    ta.focus();
    ta.setSelectionRange(offset, offset + (lines[line - 1]?.length ?? 0));
    const box = this.scroller()?.nativeElement;
    if (box) box.scrollTop = Math.max(0, (line - 3) * 20);
  }

  /** Reviews the engine's diff, then saves on confirmation. */
  protected async review(): Promise<void> {
    await this.store.review();
    const preview = this.store.preview();
    if (!preview || !this.store.canSave()) return;
    if (await reviewConfigChanges(this.dialog, { preview, maxBackups: this.maxBackups })) await this.store.save();
  }
}

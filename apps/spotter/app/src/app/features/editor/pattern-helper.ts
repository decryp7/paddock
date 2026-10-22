import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, input, output, signal, untracked, viewChild } from '@angular/core';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import type { PatternSelection, PatternTest, SchemePreset } from '../../core/api.types';
import { groupClass, groupLabel, segments } from './highlight';

type Role = PatternSelection['role'];

const ROLES: { role: Role; label: string }[] = [
  { role: 'id', label: 'Product id' },
  { role: 'major', label: 'Major' },
  { role: 'minor', label: 'Minor' },
  { role: 'patch', label: 'Patch' },
  { role: 'build', label: 'Build' },
  { role: 'channel', label: 'Channel' },
  { role: 'variant', label: 'Variant' },
];

/** How long typing is collected before the pattern is tried again. */
export const TEST_DEBOUNCE_MS = 300;

/**
 * The version pattern editor: pick a ready-made pattern, or click an example name, select the parts (product id,
 * major, minor, ...) and get a regular expression you can edit; try it on any name in the Test field. The pattern is
 * written and checked by the engine; this component only collects the selection and shows the answer.
 */
@Component({
  selector: 'sp-pattern-helper',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="field">
      <label for="scheme-preset">Start from a ready-made pattern</label>
      <select id="scheme-preset" class="input" (change)="usePreset($event)">
        <option value="">Choose…</option>
        @for (s of presets(); track s.id) {
          <option [value]="s.id">{{ s.name }}</option>
        }
      </select>
    </div>

    <div class="field">
      <label for="pattern">Pattern</label>
      <input id="pattern" class="input mono" spellcheck="false" [value]="pattern()" (input)="patternChange.emit(value($event))" placeholder="Acme-(?<id>[A-Z]+)-R(?<major>\\d+)\\.(?<minor>\\d+)" [attr.aria-invalid]="!!error()" aria-describedby="pattern-help" />
      <span id="pattern-help" class="help">
        A regular expression with named groups: <code>id</code> (the product), <code>major</code>, <code>minor</code>, <code>patch</code>, <code>build</code> (numbers),
        <code>channel</code> (beta, rc), <code>variant</code> (x64, lite). Any other name is kept as a text field. Letters around the numbers stay in the version you see.
      </span>
      @if (error(); as e) {
        <p class="error" role="alert" data-testid="pattern-error">{{ e }}</p>
      }
    </div>

    <details class="builder" [open]="builderOpen()" (toggle)="builderOpen.set(isOpen($event))">
      <summary>Build it from an example name</summary>
      @if (examples().length) {
        <p class="help">Click a name, select a part of it with the mouse, then press what it is.</p>
        <div class="examples" role="group" aria-label="Example names">
          @for (e of examples(); track e) {
            <button type="button" class="btn sm" [class.on]="e === example()" (click)="pick(e)">{{ e }}</button>
          }
        </div>
      } @else {
        <p class="help">No names yet. Fill in the source and its names appear here.</p>
      }
      @if (example(); as ex) {
        <div class="sample mono" #sample data-testid="example">{{ ex }}</div>
        <div class="roles" role="group" aria-label="What the selected text is">
          @for (r of roles; track r.role) {
            <button type="button" class="btn sm" (click)="mark(r.role)">{{ r.label }}</button>
          }
        </div>
        @if (chosen().length) {
          <ul class="chosen" aria-label="Selected parts">
            @for (c of chosen(); track $index) {
              <li><mark [class]="cls(c.role)">{{ ex.slice(c.start, c.end) }}</mark> {{ label(c.role) }} <button type="button" class="btn ghost sm" (click)="unmark($index)" [attr.aria-label]="'Remove ' + label(c.role)">×</button></li>
            }
          </ul>
        }
        @if (helperError(); as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
      }
    </details>

    <div class="field">
      <label for="pattern-test">Test it on a name</label>
      <input id="pattern-test" class="input mono" spellcheck="false" [value]="testText()" (input)="testText.set(value($event))" placeholder="Paste or type a file name" />
      @if (result(); as r) {
        @if (r.read; as read) {
          <p class="result" data-testid="test-result">
            @for (s of parts(); track $index) {
              @if (s.group; as g) { <mark [class]="cls(g)" [title]="label(g)">{{ s.text }}</mark> } @else { {{ s.text }} }
            }
            <br />
            <span class="read">
              @if (read.id) { <span class="chip">{{ read.id }}</span> }
              <strong>{{ read.version }}</strong>
              @if (read.channel) { <span class="chip">{{ read.channel }}</span> }
              @if (read.variant) { <span class="chip">{{ read.variant }}</span> }
            </span>
          </p>
        } @else if (r.ignored) {
          <p class="why" data-testid="test-result">{{ r.ignored }}</p>
        }
      }
    </div>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--pd-space-4);
    }
    .help {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .error {
      color: var(--pd-danger);
    }
    .builder summary {
      cursor: pointer;
      color: var(--pd-text-muted);
    }
    .examples,
    .roles {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-1);
      margin: var(--pd-space-2) 0;
    }
    .examples .on {
      border-color: var(--pd-accent);
    }
    .sample {
      padding: var(--pd-space-2) var(--pd-space-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      user-select: text;
      word-break: break-all;
    }
    .chosen {
      margin: var(--pd-space-2) 0 0;
      padding: 0;
      list-style: none;
      display: grid;
      gap: 2px;
    }
    mark {
      padding: 0 2px;
      border-radius: 3px;
      color: inherit;
    }
    .g-id { background: color-mix(in srgb, var(--pd-accent) 28%, transparent); }
    .g-num { background: color-mix(in srgb, #3b82f6 28%, transparent); }
    .g-channel { background: color-mix(in srgb, #d97706 30%, transparent); }
    .g-variant { background: color-mix(in srgb, #8b5cf6 28%, transparent); }
    .g-field { background: color-mix(in srgb, #64748b 28%, transparent); }
    .result {
      margin: var(--pd-space-2) 0 0;
    }
    .why {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .read {
      display: inline-flex;
      gap: var(--pd-space-1);
      align-items: center;
    }
  `,
})
export class PatternHelper {
  /** The pattern being edited. */
  readonly pattern = input.required<string>();
  /** Which scheme (a stored id) the pattern belongs to; the Test field runs it. */
  readonly missingOlder = input(false);
  readonly presets = input<readonly SchemePreset[]>([]);
  /** Names from the source, offered as examples. */
  readonly examples = input<readonly string[]>([]);
  readonly patternChange = output<string>();

  private readonly backend = inject(BACKEND);
  private readonly sample = viewChild<ElementRef<HTMLElement>>('sample');

  protected readonly roles = ROLES;
  protected readonly cls = groupClass;
  protected readonly label = groupLabel;
  protected readonly example = signal('');
  protected readonly chosen = signal<PatternSelection[]>([]);
  protected readonly helperError = signal<string | null>(null);
  protected readonly builderOpen = signal(false);
  protected readonly testText = signal('');
  private readonly tested = signal<PatternTest | null>(null);
  protected readonly result = this.tested.asReadonly();
  protected readonly error = computed(() => this.tested()?.error ?? null);
  protected readonly parts = computed(() => {
    const r = this.tested()?.read;
    return r ? segments(this.testText(), r.spans) : [];
  });

  private timer: ReturnType<typeof setTimeout> | undefined;
  private run = 0;

  constructor() {
    effect(() => {
      const pattern = this.pattern();
      const text = this.testText();
      const older = this.missingOlder();
      untracked(() => this.schedule(pattern, text, older));
    });
  }

  private schedule(pattern: string, text: string, missingOlder: boolean): void {
    clearTimeout(this.timer);
    const mine = ++this.run;
    this.timer = setTimeout(() => {
      void this.backend
        .testPattern('custom', pattern, missingOlder, text)
        .then((r) => {
          if (mine === this.run) this.tested.set(r);
        })
        .catch((e: unknown) => {
          if (mine === this.run) this.tested.set({ error: toCommandError(e).message, read: null, ignored: null });
        });
    }, TEST_DEBOUNCE_MS);
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }

  protected isOpen(e: Event): boolean {
    return (e.target as HTMLDetailsElement).open;
  }

  protected usePreset(e: Event): void {
    const id = (e.target as HTMLSelectElement).value;
    const preset = this.presets().find((p) => p.id === id);
    if (!preset) return;
    this.patternChange.emit(preset.pattern);
    this.testText.set(preset.example);
  }

  protected pick(name: string): void {
    this.example.set(name);
    this.chosen.set([]);
    this.helperError.set(null);
    this.testText.set(name);
  }

  /** Records the text selected in the example as `role` and asks the engine for the pattern. */
  protected mark(role: Role): void {
    const range = this.selectedRange();
    if (!range) {
      this.helperError.set('Select a part of the example name with the mouse first.');
      return;
    }
    const without = this.chosen().filter((c) => c.role !== role && (c.end <= range.start || c.start >= range.end));
    this.chosen.set([...without, { role, ...range }].sort((a, b) => a.start - b.start));
    this.generate();
  }

  protected unmark(i: number): void {
    this.chosen.update((list) => list.filter((_, j) => j !== i));
    if (this.chosen().length) this.generate();
  }

  private generate(): void {
    this.helperError.set(null);
    this.backend.suggestPattern(this.example(), this.chosen()).then(
      (p) => this.patternChange.emit(p),
      (e: unknown) => {
        const err = toCommandError(e);
        this.helperError.set([err.message, ...err.problems].filter((x, i, a) => a.indexOf(x) === i).join(' '));
      },
    );
  }

  /** The selection inside the example element, as UTF-16 offsets into its text. */
  private selectedRange(): { start: number; end: number } | null {
    const el = this.sample()?.nativeElement;
    const sel = el?.ownerDocument.getSelection();
    if (!el || !sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
    const r = sel.getRangeAt(0);
    if (!el.contains(r.startContainer) || !el.contains(r.endContainer)) return null;
    const offset = (node: Node, off: number) => {
      const pre = el.ownerDocument.createRange();
      pre.selectNodeContents(el);
      pre.setEnd(node, off);
      return pre.toString().length;
    };
    const start = offset(r.startContainer, r.startOffset);
    const end = offset(r.endContainer, r.endOffset);
    return end > start ? { start, end } : null;
  }
}

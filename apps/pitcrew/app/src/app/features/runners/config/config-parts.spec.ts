import { TestBed } from '@angular/core/testing';
import type { ConfigPreview } from '../../../core/api.types';
import { appProviders, byText, makeMock, text, type as typeInto } from '../../../../testing/test-helpers';
import { answered, DISMISSED } from '@decryp7/paddock-ui/shell';
import { ConfigDiffDialog, reviewConfigChanges } from './config-diff-dialog';
import { FieldIssues } from './field-issues';
import { StringList } from './string-list';

const preview: ConfigPreview = {
  text: 'a',
  changed: true,
  restartNeeded: false,
  changedKeys: [],
  labels: [],
  issues: [{ severity: 'warning', path: 'x', line: 1, message: 'Odd key.' }],
  diff: {
    added: 1,
    removed: 1,
    truncated: true,
    hunks: [
      { lines: [{ kind: 'removed', text: 'old', oldNo: 1, newNo: null }, { kind: 'added', text: 'new', oldNo: null, newNo: 1 }] },
      { lines: [{ kind: 'same', text: 'ctx', oldNo: 9, newNo: 9 }] },
    ],
  },
};

describe('config parts', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: appProviders(makeMock()) }));
  afterEach(() => document.querySelectorAll('.cdk-overlay-container > *').forEach((n) => n.remove()));

  it('FieldIssues shows the severity as a word next to the message', () => {
    const f = TestBed.createComponent(FieldIssues);
    f.componentRef.setInput('issues', [preview.issues[0], { severity: 'error', path: 'y', line: null, message: 'Bad.' }]);
    f.detectChanges();
    const paras = Array.from((f.nativeElement as HTMLElement).querySelectorAll('p'));
    expect(paras.map((p) => text(p))).toEqual(['Warning: Odd key.', 'Error: Bad.']);
    expect(paras[1]!.classList.contains('error')).toBe(true);
  });

  it('StringList adds on Enter or the button, ignores blanks and duplicates, and removes', () => {
    const f = TestBed.createComponent(StringList);
    f.componentRef.setInput('items', ['a']);
    f.componentRef.setInput('label', 'Things');
    f.detectChanges();
    const out: string[][] = [];
    f.componentInstance.itemsChange.subscribe((v) => out.push(v));
    const el = f.nativeElement as HTMLElement;
    const box = el.querySelector('input') as HTMLInputElement;
    const add = byText<HTMLButtonElement>(el, 'button', 'Add')!;
    expect(add.disabled).toBe(true);
    typeInto(box, ' b ');
    f.detectChanges();
    box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(out).toEqual([['a', 'b']]);
    typeInto(box, 'a');
    f.detectChanges();
    add.click();
    expect(out).toHaveLength(1);
    (el.querySelector('button[aria-label="Remove a"]') as HTMLElement).click();
    expect(out.at(-1)).toEqual([]);
  });

  /** The dialog's content in a fake dialog window: its inputs as the opener sends them, and the choice it closes with. */
  function mountDiff(inputs: { preview: typeof preview; maxBackups: number }) {
    const f = TestBed.createComponent(ConfigDiffDialog);
    f.componentRef.setInput('preview', inputs.preview);
    f.componentRef.setInput('maxBackups', inputs.maxBackups);
    f.detectChanges();
    return { f, d: f.nativeElement as HTMLElement, c: f.componentInstance };
  }

  it('the diff dialog shows hunks with symbols and words and notes, and declares Cancel', () => {
    const { d, c } = mountDiff({ preview, maxBackups: 10 });
    expect(text(d.querySelector('.counts'))).toContain('+1');
    expect(d.querySelectorAll('table')).toHaveLength(2);
    expect(text(d)).toContain('Removed:old');
    expect(text(d)).toContain('Added:new');
    expect(text(d)).toContain('The list of changes is cut off');
    expect(text(d)).toContain('Warning: Odd key.');
    expect(text(d)).toContain('does not need to restart');
    expect(c.dialogActions().find((a) => a.id === 'cancel')?.kind).toBe('dismiss');
  });

  it('the diff dialog says when no lines changed', async () => {
    const { d, c } = mountDiff({ preview: { ...preview, diff: { hunks: [], added: 0, removed: 0, truncated: false }, changedKeys: ['runner.capacity'], restartNeeded: true }, maxBackups: 3 });
    expect(text(d)).toContain('No lines changed');
    expect(text(d)).toContain('Changes: runner.capacity');
    expect(text(d)).toContain('the last 3 are kept');
    const save = c.dialogActions().find((a) => a.id === 'save')!;
    expect(save).toMatchObject({ kind: 'answer', label: 'Save and apply', role: 'primary' });
    expect(save.kind === 'answer' && (await save.answer(new AbortController().signal))).toBe(true);
  });

  it('opens as a window of its own and answers with the choice', async () => {
    const openDialog = vi.fn().mockResolvedValueOnce(answered(true)).mockResolvedValueOnce(DISMISSED);
    expect(await reviewConfigChanges({ openDialog }, { preview, maxBackups: 10 })).toBe(true);
    expect(openDialog).toHaveBeenCalledWith('pc.config-diff', { preview, maxBackups: 10 });
    expect(await reviewConfigChanges({ openDialog }, { preview, maxBackups: 10 })).toBe(false);
  });
});

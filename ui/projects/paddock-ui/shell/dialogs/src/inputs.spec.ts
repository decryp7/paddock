import { fakeDialogContract } from '@decryp7/paddock-ui/shell/testing';
import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { defineDialog, msg, type, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import type { DialogService } from './dialog.service';
import type { DialogInputsOf, DialogInputsOfDef } from './types';

// `open`'s inputs are typed from the content component's `input()` signals (plan §13): checked by the type-checker.

@Component({ selector: 'pd-test-rename', changeDetection: ChangeDetectionStrategy.OnPush, template: '' })
class Rename {
  readonly current = input('');
  readonly limit = input<number, string | number>(10, { transform: (v: string | number) => Number(v) });
  readonly notAnInput = 3;
}

const RENAME = defineDialog(fakeDialogContract({ id: 't.rename', kind: 'form', modality: 'modal', commands: [] }), { title: msg('t.rename', 'Rename'), result: type<string>(), load: () => Promise.resolve(Rename) });

const UNTYPED = defineDialog(fakeDialogContract({ id: 't.any', kind: 'viewer', commands: [] }), { title: msg('t.any', 'Any'), load: () => Promise.resolve(Rename as never) });

describe('dialog inputs', () => {
  it('are the component’s inputs, optional, with the transform’s write type', () => {
    expectTypeOf<DialogInputsOf<Rename>>().toEqualTypeOf<{ readonly current?: string; readonly limit?: string | number }>();
    expectTypeOf<DialogInputsOfDef<typeof RENAME>>().toEqualTypeOf<{ readonly current?: string; readonly limit?: string | number }>();
  });

  it('are typed in open() for a definition and free for an id', () => {
    const open = (s: DialogService) => {
      s.open(RENAME, { inputs: { current: 'Old name', limit: '5' } });
      // @ts-expect-error a wrong type for an input
      s.open(RENAME, { inputs: { current: 1 } });
      // @ts-expect-error not an input of the component
      s.open(RENAME, { inputs: { notAnInput: 4 } });
      s.open('t.rename', { inputs: { anything: true } });
      s.open(UNTYPED, { inputs: { anything: true } });
      expectTypeOf(s.open(RENAME).closed).toEqualTypeOf<Promise<DialogOutcome<string>>>();
    };
    expect(typeof open).toBe('function');
  });
});

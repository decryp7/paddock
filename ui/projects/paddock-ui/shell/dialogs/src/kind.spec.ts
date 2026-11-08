import { DIALOG_KIND_TABLE, DIALOG_KINDS, DIALOG_WIDTH_STEPS } from '@decryp7/paddock-ui/shell';
import { fakeDialogContract } from '@decryp7/paddock-ui/shell/testing';
import { growsOnly, sizeModeOf } from './kind';

// ADR 0026 §3 / ADR 0016 decision 7: one "is fitted" table, generated from `paddock_dialogs::Kind::policy`. Before the
// table was generated, TypeScript's `fitsContent` (now `sizeModeOf`) said a viewer with a height is fitted while Rust's `is_fitted` said it
// was not (ADR 0016 cause 6), and a form with a height was fitted in Rust's table but not in TypeScript's.
describe('the kind table', () => {
  for (const kind of DIALOG_KINDS) {
    for (const height of [null, 480]) {
      it(`fits a ${kind} ${height === null ? 'without' : 'with'} a height exactly as Rust's table says`, () => {
        const contract = fakeDialogContract({ id: 't.k', kind, height: kind === 'viewer' || kind === 'tool' ? height : null });
        expect(sizeModeOf(contract)).toBe(DIALOG_KIND_TABLE[kind].sizeMode);
        expect(growsOnly(contract)).toBe(DIALOG_KIND_TABLE[kind].shrink === 'never');
      });
    }
  }

  it('is ADR 0026 §1: three width steps, defaults by kind, a tool alone user-sized and alone remembering its size (ADR 0043 D3)', () => {
    expect(DIALOG_WIDTH_STEPS).toEqual({ narrow: 400, medium: 560, wide: 720 });
    expect(DIALOG_KINDS.map((k) => DIALOG_KIND_TABLE[k].width)).toEqual([400, 560, 720, 960]);
    expect(DIALOG_KINDS.map((k) => DIALOG_KIND_TABLE[k].sizeMode)).toEqual(['height', 'height', 'height', 'manual']);
    expect(DIALOG_KINDS.map((k) => DIALOG_KIND_TABLE[k].remembers)).toEqual(['offset', 'offset', 'offset', 'sizeAndOffset']);
  });
});

import { defineDialog, msg, type, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';

/** The id of the shared confirmation (ADR 0026 §5). */
export const CONFIRM_DIALOG_ID = 'pd.confirm';

/** What `pd.confirm` asks: plain, already localised text (it crosses to the dialog's window). */
export interface ConfirmInput {
  /** The question, as the window's title and heading ("Delete the watch?"). */
  readonly heading: string;
  /** What happens, in a sentence or two. */
  readonly message: string;
  /** Facts the user weighs (what is affected), listed under the message. */
  readonly details?: readonly string[];
  /** The affirmative: a verb and its object ("Delete watch"); `destructive` draws it as the danger action, never the Enter target. */
  readonly confirm: { readonly label: string; readonly destructive?: boolean };
  /** The way out; defaults to Cancel. */
  readonly cancelLabel?: string;
}

export interface ConfirmDialogOptions {
  /** The component: `() => import('@decryp7/paddock-ui/shell/confirm/dialog').then((m) => m.PdConfirmDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The shared confirmation (ADR 0026 §5, rule 42): a modal decision, one window per question, that replaces each app's own
 * confirm dialog. It calls nothing: it answers `true` for the affirmative or is dismissed, and the opener acts.
 */
export function defineConfirmDialog(options: ConfirmDialogOptions): DialogDef<typeof CONFIRM_DIALOG_ID, true> {
  return defineDialog(DIALOG_CONTRACTS[CONFIRM_DIALOG_ID], {
    title: msg('pd.confirm.title', 'Confirm'),
    result: type<true>(),
    load: options.load,
  });
}

/** What can open a dialog by id (the window's `DialogService`). */
export interface ConfirmOpener {
  open(id: string, options: { readonly inputs: Readonly<Record<string, unknown>>; readonly title?: string }): { readonly closed: Promise<{ readonly kind: 'answered' | 'dismissed' }> };
}

/** Asks `input` in `pd.confirm`'s window; true when the user chose the affirmative, false when they dismissed it. */
export async function confirm(dialogs: ConfirmOpener, input: ConfirmInput): Promise<boolean> {
  const outcome = await dialogs.open(CONFIRM_DIALOG_ID, { inputs: { ...input }, title: input.heading }).closed;
  return outcome.kind === 'answered';
}

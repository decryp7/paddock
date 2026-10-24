/** What a key press does in a dialog (ADR 0009 §1): Escape and Cmd/Ctrl+W dismiss it, Enter runs its default action. */
export type DialogKeyAction = 'dismiss' | 'default' | null;

/** The parts of a `KeyboardEvent` the decision needs (plain objects in tests). */
export interface DialogKeyEvent {
  readonly key: string;
  readonly metaKey: boolean;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly isComposing?: boolean;
  readonly defaultPrevented?: boolean;
  readonly target: unknown;
}

/** Elements where Enter keeps its own meaning. */
const OWN_ENTER = new Set(['TEXTAREA', 'BUTTON', 'A', 'SELECT', 'SUMMARY']);

function tagOf(target: unknown): string {
  return (target as { tagName?: string } | null)?.tagName?.toUpperCase() ?? '';
}

/** What a key press does in a dialog. Enter keeps its own meaning in a text area, button, link, list or editable text. */
export function dialogKeyAction(e: DialogKeyEvent): DialogKeyAction {
  if (e.isComposing || e.defaultPrevented) return null;
  if (e.key === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey) return 'dismiss';
  if ((e.key === 'w' || e.key === 'W') && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) return 'dismiss';
  if (e.key === 'Enter' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    const t = e.target as { isContentEditable?: boolean; getAttribute?: (n: string) => string | null } | null;
    if (OWN_ENTER.has(tagOf(e.target)) || t?.isContentEditable || t?.getAttribute?.('role') === 'option') return null;
    return 'default';
  }
  return null;
}

/** The dialog's default action: a `[data-default]` button, else the first enabled primary button. */
export function findDefaultAction(root: ParentNode): HTMLElement | null {
  return (
    root.querySelector<HTMLElement>('button[data-default]:not([disabled])') ??
    root.querySelector<HTMLElement>('button.primary:not([disabled]), button.pd-button--primary:not([disabled])')
  );
}

/** The labels of a button that closes a dialog without an answer. */
const CLOSE_LABEL = /^(close|cancel|not now|dismiss|done|later|got it)\b/i;

/**
 * Whether `root` has a visible control that closes the dialog without an answer: one marked `data-dialog-close` (the
 * explicit contract), or a button labelled Close, Cancel, Not now, ... Controls inside `except` (the shell's own footer)
 * do not count.
 */
export function hasCloseAction(root: ParentNode, except?: Element | null): boolean {
  const own = (el: Element) => !except?.contains(el);
  if ([...root.querySelectorAll('[data-dialog-close]')].some(own)) return true;
  return [...root.querySelectorAll('button')].some((b) => own(b) && CLOSE_LABEL.test((b.textContent ?? '').trim()));
}

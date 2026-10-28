import type { DialogKind, DialogModality } from '@decryp7/paddock-ui/shell';

/*
 * The declared actions of a dialog and their layout (ADR 0026 §2). A content component never draws its footer: it declares
 * its actions as data, and the frame lays them out with `layoutActions`, one pure function tested as a table. Order is
 * native per OS (user decision D1), counts are checked, and Enter and the first focus follow from the roles.
 */

/** Every action has an id (test id `dialog-action-<id>`) and an already localised label. */
export interface ActionBase {
  readonly id: string;
  readonly label: string;
  /** Default true. A disabled primary is never run by Enter. */
  readonly enabled?: boolean;
}

/** Closes the dialog with the value `answer` resolves to. A rejection keeps it open with the error and Try again. */
export interface AnswerAction<R = unknown> extends ActionBase {
  readonly kind: 'answer';
  readonly role: 'primary' | 'secondary' | 'destructive';
  readonly answer: (signal: AbortSignal) => R | Promise<R>;
  /** The work honours `signal`: Escape, Cancel or an OS close while it runs aborts it and closes with no answer. */
  readonly abortable?: boolean;
  /** At the leading edge (a secondary, or a destructive beside a primary). */
  readonly lead?: boolean;
}

/** Does something and never closes (Copy, Show cited rows). */
export interface RunAction extends ActionBase {
  readonly kind: 'run';
  readonly role: 'primary' | 'secondary';
  readonly run: (signal: AbortSignal) => void | Promise<void>;
  readonly lead?: boolean;
}

/** The one way out without an answer: Cancel (modal) or Close (modeless). */
export interface DismissAction extends ActionBase {
  readonly kind: 'dismiss';
}

export type DialogAction<R = unknown> = AnswerAction<R> | RunAction | DismissAction;

/** An error whose message is for the user, as is: the frame shows it in its alert above the body. */
export class DialogActionError extends Error {
  override readonly name = 'DialogActionError';
}

/** The OS the dialog runs on: it decides the button order (D1). */
export type DialogPlatform = 'macos' | 'windows' | 'linux';

/**
 * The OS this window runs on: the shell's `data-platform` on `<html>` (set by Rust before the page boots), else the
 * user agent (the browser build).
 */
export function dialogPlatform(
  nav: { readonly platform?: string; readonly userAgent?: string } | undefined = globalThis.navigator,
  doc: Pick<Document, 'documentElement'> | undefined = globalThis.document,
): DialogPlatform {
  const set = doc?.documentElement?.dataset?.['platform'];
  if (set === 'macos' || set === 'windows' || set === 'linux') return set;
  const p = `${nav?.platform ?? ''} ${nav?.userAgent ?? ''}`;
  if (/Mac|iPhone|iPad/.test(p)) return 'macos';
  if (/Win/.test(p)) return 'windows';
  return 'linux';
}

/** How one action is drawn. */
export interface PlacedAction {
  readonly action: DialogAction;
  /** One filled button per dialog: the primary, or a destructive that is the only affirmative (then `danger`). */
  readonly filled: boolean;
  readonly danger: boolean;
}

/** The footer of a dialog, as `layoutActions` lays it out. */
export interface ActionLayout {
  /** At the leading edge, in order. */
  readonly lead: readonly PlacedAction[];
  /** At the trailing edge, in order. */
  readonly trail: readonly PlacedAction[];
  /** The way out without an answer (declared, or the frame's own); `null` only for a required choice. */
  readonly dismiss: DismissAction | null;
  /** What Enter runs (when enabled): the primary; Cancel for a destructive decision; `null`: Enter does nothing. */
  readonly enterTarget: string | null;
  /** What has the focus first: `'field'` (a form's first field), `'heading'` (no safe action) or an action id. */
  readonly focus: 'field' | string | null;
  /**
   * A decision's icon: `danger` only when a destructive action is the sole affirmative (Delete, Remove); a primary
   * beside a destructive (Save, Don't save) is an ordinary question, `info`.
   */
  readonly tone: 'danger' | 'info';
  /** What is wrong with the declaration (empty when it is valid). A defect lays out the fallback: one Close. */
  readonly problems: readonly string[];
}

/** The labels of the frame's own dismiss action, already localised. */
export interface DismissLabels {
  readonly cancel: string;
  readonly close: string;
}

/** The id of the frame's own dismiss action. */
export const FRAME_DISMISS_ID = 'dismiss';

const ID = /^[a-z][a-z0-9-]*$/;

/**
 * The footer of a dialog of `kind` and `modality` from its declared `actions`, for `platform` (ADR 0026 §2). Pure.
 *
 * - Order: macOS and Linux `[lead] … [secondary] [cancel] [primary | destructive]`; Windows `[lead] … [primary |
 *   destructive] [secondary] [cancel]`. A destructive beside a primary is outlined: at the leading edge on macOS and
 *   Linux, between the primary and Cancel on Windows.
 * - Counts: at most one primary and one destructive; exactly one dismiss, none for a required choice; without a declared
 *   dismiss the frame adds Cancel (modal) or Close (modeless).
 * - Enter: the primary; never a destructive action (a destructive decision's Enter is Cancel).
 * - Focus: a form's first field; Cancel when a destructive action is the sole affirmative; anything else the Enter
 *   target (the save prompt's Save); never a destructive action (a required choice that only destroys focuses its heading).
 * - Tone: `danger` only when a destructive action is the sole affirmative, else `info`.
 * - A declaration that breaks a rule never throws: the layout is one Close, and `problems` says why.
 */
export function layoutActions(
  kind: DialogKind,
  modality: DialogModality,
  requiredChoice: boolean,
  actions: readonly DialogAction[],
  platform: DialogPlatform,
  labels: DismissLabels,
): ActionLayout {
  const problems: string[] = [];
  const ids = new Set<string>();
  for (const a of actions) {
    if (!a || typeof a !== 'object' || !['answer', 'run', 'dismiss'].includes(a.kind)) {
      problems.push('an action is not an answer, run or dismiss action');
      continue;
    }
    if (typeof a.id !== 'string' || !ID.test(a.id)) problems.push(`action id ${JSON.stringify(a.id)} is not lower-case letters, digits and '-'`);
    else if (ids.has(a.id)) problems.push(`action id ${a.id} is declared twice`);
    else ids.add(a.id);
    if (typeof a.label !== 'string' || !a.label.trim()) problems.push(`action ${a.id} has no label`);
    if (a.kind !== 'dismiss' && !['primary', 'secondary', 'destructive'].includes(a.role)) problems.push(`action ${a.id} has no role`);
    if (a.kind === 'run' && (a.role as string) === 'destructive') problems.push(`run action ${a.id} cannot be destructive (it never closes)`);
  }
  const valid = actions.filter((a) => a && typeof a === 'object' && ['answer', 'run', 'dismiss'].includes(a.kind));
  const primaries = valid.filter((a) => a.kind !== 'dismiss' && a.role === 'primary');
  const destructives = valid.filter((a) => a.kind === 'answer' && a.role === 'destructive');
  const dismisses = valid.filter((a): a is DismissAction => a.kind === 'dismiss');
  if (primaries.length > 1) problems.push(`${primaries.length} primary actions (at most one)`);
  if (destructives.length > 1) problems.push(`${destructives.length} destructive actions (at most one)`);
  if (requiredChoice && dismisses.length) problems.push('a required choice has no dismiss action');
  if (dismisses.length > 1) problems.push(`${dismisses.length} dismiss actions (exactly one)`);
  if (requiredChoice && !valid.some((a) => a.kind === 'answer')) problems.push('a required choice needs an answer');

  const frameDismiss: DismissAction = { kind: 'dismiss', id: FRAME_DISMISS_ID, label: modality === 'modal' ? labels.cancel : labels.close };
  if (problems.length) {
    const close: DismissAction = { kind: 'dismiss', id: FRAME_DISMISS_ID, label: labels.close };
    return { lead: [], trail: [{ action: close, filled: false, danger: false }], dismiss: close, enterTarget: null, focus: close.id, tone: 'info', problems };
  }

  const dismiss = requiredChoice ? null : (dismisses[0] ?? frameDismiss);
  const primary = primaries[0] ?? null;
  const destructive = destructives[0] ?? null;
  const both = primary !== null && destructive !== null;
  const place = (action: DialogAction): PlacedAction => ({
    action,
    filled: action === primary || (action === destructive && !both),
    danger: action === destructive && !both,
  });
  const secondaries = valid.filter((a) => a.kind !== 'dismiss' && a.role === 'secondary');
  const leadSecondaries = secondaries.filter((a) => a.kind !== 'dismiss' && a.lead === true);
  const trailSecondaries = secondaries.filter((a) => !leadSecondaries.includes(a));
  const affirmative = primary ?? destructive;
  const lead: DialogAction[] = [...leadSecondaries];
  const trail: DialogAction[] = [];
  if (platform === 'windows') {
    if (affirmative) trail.push(affirmative);
    if (both && destructive) trail.push(destructive);
    trail.push(...trailSecondaries);
    if (dismiss) trail.push(dismiss);
  } else {
    if (both && destructive) lead.push(destructive);
    trail.push(...trailSecondaries);
    if (dismiss) trail.push(dismiss);
    if (affirmative) trail.push(affirmative);
  }
  const enterTarget = primary ? primary.id : destructive && dismiss ? dismiss.id : null;
  // A destructive action that is the sole affirmative: Cancel has the focus and the decision's tone is danger.
  const destructiveDecision = kind === 'decision' && destructive !== null && !both;
  // Never a destructive action: a required choice whose only answer destroys something focuses its heading instead.
  const safe = (a: DialogAction | null | undefined) => (a && !(a.kind === 'answer' && a.role === 'destructive') ? a.id : null);
  const firstSafe = valid.find((a) => a.kind !== 'dismiss' && safe(a) !== null)?.id ?? null;
  const focus = kind === 'form' ? 'field' : destructiveDecision && dismiss ? dismiss.id : (enterTarget ?? dismiss?.id ?? safe(affirmative) ?? firstSafe ?? 'heading');
  const tone = destructive !== null && !both ? 'danger' : 'info';
  return { lead: lead.map(place), trail: trail.map(place), dismiss, enterTarget, focus, tone, problems };
}

/** What a key press does in a dialog (ADR 0009 §1): Escape and Cmd/Ctrl+W dismiss it, Enter runs its Enter target. */
export type DialogKeyAction = 'dismiss' | 'enter' | null;

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

/** What a key press does in a dialog. Enter keeps its own meaning in a text area, button, link, list or editable text. */
export function dialogKeyAction(e: DialogKeyEvent): DialogKeyAction {
  if (e.isComposing || e.defaultPrevented) return null;
  if (e.key === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey) return 'dismiss';
  if ((e.key === 'w' || e.key === 'W') && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) return 'dismiss';
  if (e.key === 'Enter' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    const t = e.target as { tagName?: string; isContentEditable?: boolean; getAttribute?: (n: string) => string | null } | null;
    const tag = t?.tagName?.toUpperCase() ?? '';
    if (OWN_ENTER.has(tag) || t?.isContentEditable || t?.getAttribute?.('role') === 'option') return null;
    return 'enter';
  }
  return null;
}

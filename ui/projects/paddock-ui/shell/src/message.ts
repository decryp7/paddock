/**
 * A user-visible text of a contribution with its stable message id (`pw.models.refresh`). The id is kept next to the
 * English text so the lint can check it at run time and the shell's i18n (plan S15) can translate by id; `$localize`
 * output is a plain string and loses its id, which is why contributions take a `Message` and not a string.
 */
export interface Message {
  /** Namespaced message id without the `@@` prefix: `pd.` for the shell, the app's prefix (`pw.`, `db.`) for apps. */
  readonly id: string;
  /** The source (English) text. */
  readonly text: string;
}

/** Shape of an id: a namespace, then dot-separated lower-case parts with hyphens (`pw.models.refresh`, `sd.say-hello`). */
export const CONTRIBUTION_ID = /^[a-z][a-z0-9]*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/;

/** A message with its id: `msg('pw.models.refresh', 'Refresh models')`. Frozen; validated by the lint, not here. */
export function msg(id: string, text: string): Message {
  return Object.freeze({ id, text });
}

/** Whether `value` is a usable message: a well-formed id and a non-blank text. */
export function isMessage(value: unknown): value is Message {
  if (typeof value !== 'object' || value === null) return false;
  const m = value as Partial<Message>;
  return typeof m.id === 'string' && CONTRIBUTION_ID.test(m.id) && typeof m.text === 'string' && m.text.trim().length > 0;
}

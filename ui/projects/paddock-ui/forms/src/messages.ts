/** The pure parts of the forms helpers (plan X21, rule 37: new forms use Signal Forms): error words and when to show them. */

/** What an error needs to be turned into words: Signal Forms' `ValidationError`, with the limits its built-in kinds carry. */
export interface PdFieldError {
  readonly kind: string;
  readonly message?: string;
}

/** The part of a Signal Forms field state the helpers read (the call result of `form.name()`). */
export interface PdFieldStateLike {
  errors(): readonly PdFieldError[];
  touched(): boolean;
  invalid?(): boolean;
  pending?(): boolean;
  required?(): boolean;
  disabled?(): boolean;
}

/** The words for an error kind: a string, or a function of the error (for limits). Beats the defaults. */
export type PdFieldMessages = Readonly<Record<string, string | ((error: PdFieldError) => string)>>;

/** A limit the built-in error kinds carry (`min`, `maxLength`...); 0 when the error has none. */
const num = (e: PdFieldError, key: string): number => {
  const v = (e as unknown as Record<string, unknown>)[key];
  return typeof v === 'number' ? v : 0;
};

/** The words for the built-in kinds: plain, sentence case, what to do rather than what is wrong. */
export function defaultErrorText(e: PdFieldError): string {
  switch (e.kind) {
    case 'required':
      return $localize`:@@pd.forms.required:This is required.`;
    case 'min':
      return $localize`:@@pd.forms.min:Enter ${num(e, 'min')}:min: or more.`;
    case 'max':
      return $localize`:@@pd.forms.max:Enter ${num(e, 'max')}:max: or less.`;
    case 'minLength':
      return $localize`:@@pd.forms.minLength:Use at least ${num(e, 'minLength')}:min: characters.`;
    case 'maxLength':
      return $localize`:@@pd.forms.maxLength:Use at most ${num(e, 'maxLength')}:max: characters.`;
    case 'pattern':
      return $localize`:@@pd.forms.pattern:This is not in the expected format.`;
    case 'email':
      return $localize`:@@pd.forms.email:Enter a valid email address.`;
    default:
      return $localize`:@@pd.forms.invalid:This value is not valid.`;
  }
}

/** The words for one error: the error's own message (an engine refusal says what to fix), then the app's, then the default. */
export function errorText(error: PdFieldError, messages: PdFieldMessages = {}): string {
  if (error.message) return error.message;
  const custom = messages[error.kind];
  return typeof custom === 'function' ? custom(error) : (custom ?? defaultErrorText(error));
}

/**
 * Whether the field's errors are shown: after the user left the field (touched) or tried to submit, never while an async
 * check is still running, and not for a disabled field. Nothing shouts at someone who is still typing the first character.
 */
export function showErrors(state: PdFieldStateLike, submitted = false): boolean {
  if (state.disabled?.() || state.pending?.()) return false;
  return (state.touched() || submitted) && state.errors().length > 0;
}

/** The one message a field shows (the first error), or null while errors are hidden or there are none. */
export function fieldMessage(state: PdFieldStateLike | null, messages: PdFieldMessages = {}, submitted = false): string | null {
  if (!state || !showErrors(state, submitted)) return null;
  const first = state.errors()[0];
  return first ? errorText(first, messages) : null;
}

/**
 * `@decryp7/paddock-ui/forms`: helpers for Signal Forms (plan X21, rule 37): `pd-field` (label, hint and one error with the alert
 * icon at 14 px, wired to the control through `pdFieldControl`) and the pure validation display (`fieldMessage`, `showErrors`,
 * `errorText`): when errors show (after the user left the field, or on submit) and the words for the built-in kinds.
 */
export { PdField, PdFieldControl } from './src/field';
export { defaultErrorText, errorText, fieldMessage, showErrors, type PdFieldError, type PdFieldMessages, type PdFieldStateLike } from './src/messages';

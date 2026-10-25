/**
 * `@decryp7/paddock-ui/states`: presentational states every app shows the same way. Today: `<pd-needs-ai-card>`, the one look of
 * "this feature needs local AI" (ADR 0004). It knows nothing about Pitwall: `<pd-needs-ai>` (`shell/pitwall`) fills it from the
 * engine's state, and `<pd-view>` fills it with a plugin's reason when the host has no Pitwall state. One look, one place to change it.
 */
export { PdNeedsAiCard, type NeedsAiIcon } from './src/needs-ai-card';

/**
 * Layout specs that measure text pin the longest realistic value (ER-12; user decision 2026-11-21, after Dyno's "Memory 24.8 of 48 GB"
 * label needed 135 px in a 132 px box in run 8446: a live sample value can be wider than the sample the spec happened to see).
 * A spec that measures text width, a cut-off or a wrapped label is pinned when it carries a marker: a call to `pinLongestValues(`
 * or a `layout-pinned:` comment naming the longest value it pins. This check is report-only: it lists the specs without one.
 */

/** What makes a spec a text measurement: the shared text checks or a direct read of a text box's width. */
const MEASURES = /\b(textClippedWithoutEllipsis|textCutOffWithoutTooltip|expectNoSilentClipping|expectNoButtonLabelWraps|buttonLabelsThatWrap|scrollWidth|expectWithinWidth)\b/;
/** What pins the longest value. */
const PINNED = /\bpinLongestValues\(|layout-pinned:/;

/** A spec file: its path and its text. */
export interface SpecSource {
  readonly path: string;
  readonly source: string;
}

/** The paths of the specs that measure text but pin no longest value, in the given order. */
export function unpinnedLayoutSpecs(specs: readonly SpecSource[]): string[] {
  return specs.filter((s) => MEASURES.test(s.source) && !PINNED.test(s.source)).map((s) => s.path);
}

// Generated from paddock-dialogs (the kind table of ADR 0026 §1). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-dialogs --lib generated.

/** The three width steps, logical px (ADR 0026 §1, D2). */
export const DIALOG_WIDTH_STEPS = { narrow: 400, medium: 560, wide: 720 } as const;
export type DialogWidthStep = keyof typeof DIALOG_WIDTH_STEPS;

/** The least a dialog window can be, logical px. */
export const DIALOG_MIN_SIZE = { width: 320, height: 200 } as const;

/** The largest share of the work area a dialog's preferred size takes; never below its minimum content. */
export const DIALOG_CAP_FRACTION = 0.75;

/**
 * What each kind fixes: modalities, the default width and preferred height (`null`: measured), its floor, whether it
 * is fitted to its content (`fits`, the one "is fitted" table of ADR 0016 decision 7), whether a refit only grows, and
 * what it remembers between openings.
 */
export const DIALOG_KIND_TABLE = {
  decision: { modalities: ['modal'], width: 400, height: null, minSize: null, fits: true, growsOnly: false, remembers: 'offset', rememberHeight: false },
  form: { modalities: ['modal', 'modeless'], width: 560, height: null, minSize: null, fits: true, growsOnly: false, remembers: 'widthAndOffset', rememberHeight: false },
  viewer: { modalities: ['modeless'], width: 720, height: 560, minSize: { width: 680, height: 560 }, fits: true, growsOnly: true, remembers: 'sizeAndOffset', rememberHeight: true },
  tool: { modalities: ['modeless'], width: 960, height: 680, minSize: null, fits: false, growsOnly: false, remembers: 'sizeAndOffset', rememberHeight: true },
} as const;

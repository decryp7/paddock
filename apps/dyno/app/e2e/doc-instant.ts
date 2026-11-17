/**
 * The one instant of every documentation picture and of the spec that reads their text (screens-consistency.spec.ts): the pinned clock starts at
 * FROZEN (14:30:00 UTC) and a picture is taken SHOT_AFTER_MS later, in the first minute, so the clock the pages show (14:30), the page clock and the
 * minute the mock's polled replies read (`?stable=60000`, mock/mac-machine.ts) are one and the same.
 */
export const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);
/** How long after FROZEN a picture is taken (half a minute: the middle of the minute the mock reads). */
export const SHOT_AFTER_MS = 30_000;

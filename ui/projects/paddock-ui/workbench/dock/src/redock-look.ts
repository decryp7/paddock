import type { RedockLook } from '@decryp7/paddock-ui/workbench';

/**
 * A pop-out window dragged back over its host turns translucent there (the shell does it, per OS), so the drop guides
 * under it show; the host only follows along. Pure: what the guides look like for each look of the window.
 */

/** The state of the drop guides while a pop-out is dragged over the window. */
export interface RedockGuides {
  /** The guides draw a clear outline (the system reduces transparency, so the window cannot be seen through). */
  outline: boolean;
}

/** The guides for a look of the dragged window. Every look but `outline` leaves them plain (the end of a drag is `opaque`). */
export function redockGuides(look: RedockLook): RedockGuides {
  return { outline: look === 'outline' };
}

/** The attribute on the guides layer that styles them (`dock-styles.ts`); absent when the guides are plain. */
export const REDOCK_ATTRIBUTE = 'data-redock';

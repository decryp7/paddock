/**
 * The window's regions (title bar, sidebar, page, status bar; the workbench adds its dock groups): landmarks that F6 and
 * Shift+F6 cycle, the desktop-IDE convention (ported from Debrief's `shell/regions.ts`). A frame marks each with
 * `data-pd-region`; only regions on screen count.
 */
export const REGION_ATTR = 'data-pd-region';

/** The keys the frame handles itself; `provideShellPages` reserves them so a user binding cannot take them. */
export const REGION_KEYS = [
  {
    key: 'F6',
    get description(): string {
      return $localize`:@@pd.shell.region.next:Focus next region`;
    },
  },
  {
    key: 'Shift+F6',
    get description(): string {
      return $localize`:@@pd.shell.region.previous:Focus previous region`;
    },
  },
] as const;

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Whether an element takes up room on screen (a closed sidebar does not). */
export function shown(el: HTMLElement): boolean {
  return el.getClientRects().length > 0;
}

/** The regions under `root` in document order, those on screen only. */
export function regionsOf(root: ParentNode, isShown: (el: HTMLElement) => boolean = shown): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(`[${REGION_ATTR}]`)].filter((el) => isShown(el));
}

/** The index of the region holding `el`, or -1. */
export function regionIndex(regions: readonly HTMLElement[], el: Element | null): number {
  return el ? regions.findIndex((r) => r.contains(el)) : -1;
}

/**
 * Where focus goes in a region: the current item (`aria-current`, the page of the sidebar), else its first control,
 * else the region itself (it needs `tabindex="-1"`).
 */
export function regionTarget(region: HTMLElement): HTMLElement {
  return region.querySelector<HTMLElement>('[aria-current="page"]') ?? region.querySelector<HTMLElement>(FOCUSABLE) ?? region;
}

/**
 * Moves focus to the next (1) or previous (-1) region from where focus is now, wrapping around; from outside every
 * region, the first (or last). Returns whether there was a region to go to.
 */
export function cycleRegions(root: ParentNode, direction: 1 | -1, active: Element | null, isShown: (el: HTMLElement) => boolean = shown): boolean {
  const regions = regionsOf(root, isShown);
  if (!regions.length) return false;
  const at = regionIndex(regions, active);
  const next = at < 0 ? (direction === 1 ? 0 : regions.length - 1) : (at + direction + regions.length) % regions.length;
  regionTarget(regions[next] as HTMLElement).focus();
  return true;
}

/** The direction an F6 key press moves (1, -1), or null for any other key. */
export function regionKey(e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey'>): 1 | -1 | null {
  if (e.key !== 'F6' || e.ctrlKey || e.altKey || e.metaKey) return null;
  return e.shiftKey ? -1 : 1;
}

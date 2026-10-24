import { planLayout, type OpenView } from '@decryp7/paddock-ui';

/**
 * How the sidebar is shown: `expanded` (icons and names, 216 px), `rail` (icons only, 56 px) or `overlay` (not beside the
 * page; the title bar's button opens it over the page, behind a scrim).
 */
export type SidebarMode = 'expanded' | 'rail' | 'overlay';

/** The sizes the sidebar plan works with (px). */
export interface SidebarSizes {
  /** The sidebar with names (design guide §2). */
  readonly expanded: number;
  /** The icon rail. */
  readonly rail: number;
  /** The least a page needs beside the expanded sidebar. */
  readonly pageMin: number;
  /** The least a page needs beside the rail; narrower, the sidebar goes over the page. */
  readonly compactPageMin: number;
}

/** Expanded from 776 px, the rail from 400 px, over the page below that. */
export const SIDEBAR_SIZES: SidebarSizes = Object.freeze({ expanded: 216, rail: 56, pageMin: 560, compactPageMin: 344 });

/** The sidebar and the page as views of the layout plan (ADR 0011): the page is the anchor, the sidebar a left column. */
function views(sidebar: number, pageMin: number): OpenView[] {
  return [
    { id: 'page', layout: { area: 'document', min: { width: pageMin, height: 0 }, pref: { width: pageMin, height: 0 }, priority: 100 } },
    { id: 'sidebar', layout: { area: 'side', edge: 'left', min: { width: sidebar, height: 0 }, pref: { width: sidebar, height: 0 }, priority: 80 } },
  ];
}

function fits(width: number, sidebar: number, pageMin: number): boolean {
  return !planLayout({ width, height: 0 }, views(sidebar, pageMin)).hidden.some((h) => h.id === 'sidebar');
}

/**
 * What the layout plan leaves room for in a frame `width` px wide: the expanded sidebar beside a page of at least
 * `pageMin`, else the rail beside a page of at least `compactPageMin`, else neither (`overlay`). Pure, so a window's
 * width decides it the same way every time (no flapping: the frame's width does not depend on the mode).
 */
export function sidebarFit(width: number, sizes: SidebarSizes = SIDEBAR_SIZES): SidebarMode {
  if (fits(width, sizes.expanded, sizes.pageMin)) return 'expanded';
  if (fits(width, sizes.rail, sizes.compactPageMin)) return 'rail';
  return 'overlay';
}

/** The mode shown: what fits, except that a user who collapsed the sidebar gets the rail where the expanded one fits. */
export function sidebarMode(fit: SidebarMode, collapsed: boolean): SidebarMode {
  return fit === 'expanded' && collapsed ? 'rail' : fit;
}

/** The sizes with a frame's overrides, each a finite px value of at least 0 (else the default). */
export function sidebarSizes(overrides: Partial<SidebarSizes> = {}): SidebarSizes {
  const pick = (k: keyof SidebarSizes) => {
    const v = overrides[k];
    return typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : SIDEBAR_SIZES[k];
  };
  return Object.freeze({ expanded: pick('expanded'), rail: pick('rail'), pageMin: pick('pageMin'), compactPageMin: pick('compactPageMin') });
}

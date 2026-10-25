/**
 * Menus must never be painted at the page origin before they are positioned (review 2, deviation 3 / P3-16): an overlay
 * that is positioned one frame after it is attached (CDK's connected position strategy) flashes at (0, 0) otherwise.
 * `hideUntilPlaced` keeps an element `visibility: hidden` from the moment it exists; the returned `reveal` shows it once it
 * has its position. The dock's own menu uses `placeMenu`; the Explorer's CDK context menu calls
 * `hideUntilPlaced(overlayRef.overlayElement)` right after `attach` and `reveal()` from `overlayRef.positionChanges`
 * (first emission) or `afterNextRender`.
 */

/** Keeps `el` hidden (it still lays out, so it can be measured) until the returned function is called. */
export function hideUntilPlaced(el: HTMLElement): () => void {
  const before = el.style.visibility;
  el.style.visibility = 'hidden';
  return () => {
    el.style.visibility = before;
  };
}

/** Whether a measured box sits at the page origin, where an unpositioned overlay is painted. */
export function isAtOrigin(rect: Pick<DOMRect, 'left' | 'top'>): boolean {
  return rect.left === 0 && rect.top === 0;
}

/** The top-left corner for a `width` x `height` menu opened at (`x`, `y`): moved just far enough to stay inside the viewport. */
export function clampToViewport(x: number, y: number, width: number, height: number, viewport: { width: number; height: number }, margin = 4): { x: number; y: number } {
  const fit = (at: number, size: number, room: number) => Math.max(margin, Math.min(at, room - size - margin));
  return { x: fit(x, width, viewport.width), y: fit(y, height, viewport.height) };
}

/** Puts a fixed-position menu at (`x`, `y`) kept inside the window and marks it `data-placed` (the stylesheet shows it from then on). */
export function placeMenu(menu: HTMLElement, x: number, y: number, viewport = { width: window.innerWidth, height: window.innerHeight }): void {
  const box = menu.getBoundingClientRect();
  const at = clampToViewport(x, y, box.width, box.height, viewport);
  menu.style.left = `${at.x}px`;
  menu.style.top = `${at.y}px`;
  menu.dataset['placed'] = '';
}

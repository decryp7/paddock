import { glyphSvg } from './dock-glyph';
/**
 * A group never scrolls sideways. Focusing something wider than a group (a dialog opened from a narrow tool window)
 * would scroll the group's box (overflow: hidden still scrolls programmatically) and push its tab strip over the
 * neighbouring group's header. Vertical scrolling is left alone (a short query bar shows its status that way).
 */
export function keepGroupsUnscrolled(root: HTMLElement): { dispose(): void } {
  const onScroll = (e: Event) => {
    const t = e.target;
    if (t instanceof HTMLElement && t.classList.contains('dv-groupview') && t.scrollLeft !== 0) t.scrollLeft = 0;
  };
  root.addEventListener('scroll', onScroll, true);
  return { dispose: () => root.removeEventListener('scroll', onScroll, true) };
}

/**
 * Dockview marks floating groups `role="dialog"` with `aria-modal="false"` and a stacking
 * `aria-level` (0, 1, ...). axe rejects both on a dialog (aria-level is not allowed there and 0 is
 * invalid; aria-modal=false is the default), so they are removed as groups are floated or restored.
 */
export function observeFloatingAria(root: HTMLElement): { dispose(): void } {
  const fix = () =>
    root.querySelectorAll('.dv-resize-container[aria-modal], .dv-resize-container[aria-level]').forEach((el) => {
      el.removeAttribute('aria-modal');
      el.removeAttribute('aria-level');
    });
  const observer = new MutationObserver(fix);
  observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['aria-modal', 'aria-level'] });
  fix();
  return { dispose: () => observer.disconnect() };
}

/** The name and tooltip of a tab strip's overflow chip. */
export function overflowLabel(hidden: number): string {
  return hidden === 1
    ? $localize`:Tab strip overflow chip, one hidden tab@@pd.wb.dock.overflow.one:1 more tab`
    : $localize`:Tab strip overflow chip, several hidden tabs@@pd.wb.dock.overflow.many:${hidden}:count: more tabs`;
}

/** The chip's tooltip: the count and the titles of the tabs the strip hides ("1 more tab: Jobs"). */
export function overflowTooltip(hidden: number, titles: readonly string[]): string {
  return titles.length ? `${overflowLabel(hidden)}: ${titles.join(', ')}` : overflowLabel(hidden);
}

/** Titles of the tabs of the chip's group that the strip does not show entirely (the ones dockview lists in the chip). */
function hiddenTabTitles(chip: HTMLElement): string[] {
  const strip = chip.closest('.dv-tabs-and-actions-container')?.querySelector<HTMLElement>('.dv-tabs-container');
  if (!strip) return [];
  const box = strip.getBoundingClientRect();
  return Array.from(strip.querySelectorAll<HTMLElement>('.dv-tab'))
    .filter((tab) => {
      const r = tab.getBoundingClientRect();
      return r.left < box.left || r.right > box.right;
    })
    .map((tab) => tab.querySelector('.pd-dock-tab-label')?.textContent?.trim() ?? '')
    .filter((title) => title !== '');
}

/**
 * dockview's overflow chip ("⌄ 2", the tabs a narrow strip hides) is a bare `div` that opens its list on click. It is
 * made a button here: named and titled "2 more tabs", focusable, and opened with Enter or Space (at the chip, since
 * dockview places the list at the click).
 */
export function labelTabOverflow(root: HTMLElement): { dispose(): void } {
  const label = () =>
    root.querySelectorAll<HTMLElement>('.dv-tabs-overflow-dropdown-root').forEach((chip) => {
      const hidden = Number.parseInt(chip.textContent?.trim() ?? '', 10);
      const count = Number.isFinite(hidden) ? hidden : 0;
      const name = overflowLabel(count);
      const tip = overflowTooltip(count, hiddenTabTitles(chip));
      if (chip.getAttribute('aria-label') !== name) chip.setAttribute('aria-label', name);
      if (chip.title !== tip) chip.title = tip;
      if (chip.getAttribute('role') === 'button') return;
      chip.setAttribute('role', 'button');
      chip.tabIndex = 0;
      // The chip points down like the list it opens (dockview draws a right-pointing chevron).
      // (dockview's own style turns its right-pointing chevron by 90 degrees; `.pd-dock-overflow-chevron` undoes that for ours.)
      const chevron = glyphSvg('chevron-down', 14);
      chevron.classList.add('pd-dock-overflow-chevron');
      chip.querySelector('svg')?.replaceWith(chevron);
      chip.querySelectorAll('svg, span').forEach((el) => el.setAttribute('aria-hidden', 'true'));
    });
  const onKey = (e: KeyboardEvent) => {
    const chip = e.target instanceof HTMLElement && e.target.classList.contains('dv-tabs-overflow-dropdown-root') ? e.target : null;
    if (!chip || (e.key !== 'Enter' && e.key !== ' ')) return;
    e.preventDefault();
    const r = chip.getBoundingClientRect();
    // After this key event: the open list closes on Enter, and would see this one.
    setTimeout(() => chip.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: r.left, clientY: r.bottom })));
  };
  const observer = new MutationObserver(label);
  observer.observe(root, { childList: true, subtree: true, characterData: true });
  root.addEventListener('keydown', onKey);
  // Which tabs are hidden changes with the width, not with the DOM: refresh the tooltip as the pointer or focus arrives.
  root.addEventListener('mouseover', label);
  root.addEventListener('focusin', label);
  label();
  return {
    dispose: () => {
      observer.disconnect();
      root.removeEventListener('keydown', onKey);
      root.removeEventListener('mouseover', label);
      root.removeEventListener('focusin', label);
    },
  };
}

/**
 * The tab you are looking at stays visible: when a strip is too narrow for its tabs, the active tab is scrolled into the
 * strip and the others are the ones the overflow chip lists. Re-checked when the active tab changes (class changes), tabs
 * come and go, the strip resizes, and on `refresh()` (the dock calls it after a layout change). Never scrolls a strip whose
 * active tab is already entirely inside it.
 */
export function keepActiveTabVisible(root: HTMLElement): { refresh(): void; dispose(): void } {
  const refresh = () =>
    root.querySelectorAll<HTMLElement>('.dv-tabs-container').forEach((strip) => {
      const tab = strip.querySelector<HTMLElement>('.dv-tab.dv-active-tab');
      if (!tab) return;
      const box = strip.getBoundingClientRect();
      const at = tab.getBoundingClientRect();
      if (at.left < box.left) strip.scrollLeft += at.left - box.left;
      else if (at.right > box.right) strip.scrollLeft += Math.min(at.right - box.right, at.left - box.left);
    });
  const observer = new MutationObserver(refresh);
  observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] });
  const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(refresh);
  resize?.observe(root);
  refresh();
  return {
    refresh,
    dispose: () => {
      observer.disconnect();
      resize?.disconnect();
    },
  };
}

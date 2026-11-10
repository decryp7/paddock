import { expect } from 'vitest';

/** Roles that prohibit naming (WAI-ARIA 1.2); a div or span with no role is `generic`. */
const PROHIBITED = new Set(['generic', 'none', 'presentation', 'paragraph', 'caption', 'code', 'deletion', 'emphasis', 'insertion', 'mark', 'strong', 'subscript', 'superscript', 'time']);

/**
 * The property axe's aria-prohibited-attr checks: an element with an aria-label (or aria-labelledby) has a role that allows a name,
 * or is hidden from assistive tech. Fails for a labelled roleless div.
 */
export function expectNameAllowed(el: Element | null): void {
  expect(el).not.toBeNull();
  const e = el as Element;
  if (e.closest('[aria-hidden="true"]')) return;
  const role = e.getAttribute('role');
  const named = e.hasAttribute('aria-label') || e.hasAttribute('aria-labelledby');
  expect(named).toBe(true);
  expect(role === null || PROHIBITED.has(role)).toBe(false);
}

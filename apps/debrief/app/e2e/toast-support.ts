import { expect, Locator, Page } from '@playwright/test';
import { clickWithTimePaused } from './app';

/**
 * Toasts (the notification host's stack). A toast that is not an error leaves after its time (TOAST_MS, 8 s) and the stack moves as toasts come
 * and go, so specs find toasts only here (the lint rule `paddock/e2e-transient-wait`): `heldToast` waits for one and holds it while the spec
 * reads it (the pointer on a toast stops its time, notification-host.ts), `pressInToast` presses a button in one with the clock paused.
 */

/** Toasts whose text contains `text` (every toast without it): for a count or a text check, never for a press. */
export function toastNamed(page: Page, text?: string | RegExp): Locator {
  const all = page.getByTestId('notification');
  return text === undefined ? all : all.filter({ hasText: text });
}

/**
 * Waits for the toast with `text` and holds it: the pointer rests on it, so it stays while the spec reads or presses it (debrief.yml run 8297:
 * the steps between a toast and its View took longer than its 8 s). `toast` narrows the locator (`.last()`, a visible one).
 */
export async function heldToast(page: Page, text: string | RegExp, options: { timeout?: number; pick?: (toasts: Locator) => Locator } = {}): Promise<Locator> {
  const toasts = toastNamed(page, text);
  const toast = options.pick ? options.pick(toasts) : toasts.first();
  await expect(toast).toBeVisible(options.timeout === undefined ? {} : { timeout: options.timeout });
  await toast.hover();
  return toast;
}

/** Presses `button` (a locator inside `toast`) with the toast held and the clock paused: a new toast cannot move the stack under the pointer (runs 7941, 8004). */
export async function pressInToast(page: Page, toast: Locator, button: Locator): Promise<void> {
  await toast.hover();
  await clickWithTimePaused(page, button);
}

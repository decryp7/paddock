/** Returns `value`, failing the test with a clear message (not a later TypeError) when it is `null` or `undefined`. */
export function must<T>(value: T | null | undefined, what = 'value'): T {
  if (value === null || value === undefined) throw new Error(`expected ${what} to be present`);
  return value;
}

/** Returns `items[index]` (negative counts from the end), failing the test when there is no such element. */
export function at<T>(items: ArrayLike<T>, index: number): T {
  const i = index < 0 ? items.length + index : index;
  if (i < 0 || i >= items.length) throw new Error(`expected an element at index ${index} (length ${items.length})`);
  return items[i] as T;
}

/** The text of an element with whitespace collapsed; '' for none. */
export function text(el: Node | null | undefined): string {
  return (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** The first element matching `selector`; fails the test when there is none. */
export function one<E extends Element = HTMLElement>(root: ParentNode, selector: string): E {
  const found = root.querySelector<E>(selector);
  if (!found) throw new Error(`no element for ${selector}`);
  return found;
}

/** Every element matching `selector`. */
export function all<E extends Element = HTMLElement>(root: ParentNode, selector: string): E[] {
  return [...root.querySelectorAll<E>(selector)];
}

/** The first element of `selector` whose text equals (string) or matches (RegExp) `content`, or null. */
export function byText<E extends Element = HTMLElement>(root: ParentNode, selector: string, content: string | RegExp): E | null {
  return all<E>(root, selector).find((el) => (typeof content === 'string' ? text(el) === content : content.test(text(el)))) ?? null;
}

/** The button whose text (or `aria-label`) equals / matches `label`; fails the test when there is none. */
export function buttonByText(root: ParentNode, label: string | RegExp): HTMLButtonElement {
  const match = (s: string) => (typeof label === 'string' ? s === label : label.test(s));
  const found = all<HTMLButtonElement>(root, 'button').find((b) => match(text(b)) || match(b.getAttribute('aria-label') ?? ''));
  if (!found) throw new Error(`no button "${String(label)}"`);
  return found;
}

/** Sets a field's value and fires the event a user's typing fires (`input` by default; `change` for a select). */
export function typeInto(field: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string, event = 'input'): void {
  field.value = value;
  field.dispatchEvent(new Event(event, { bubbles: true }));
}

/** A key press on `target`: keydown (cancelable, bubbling). Returns the event so a test can check `defaultPrevented`. */
export function press(target: EventTarget, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

/**
 * Animation frames run by hand: `flush()` runs every frame requested so far (a component that coalesces pointer moves per
 * frame is flushed before its result is read); `restore()` puts the real ones back. Call `restore()` in `afterEach`.
 */
export function manualFrames(): { flush(): void; readonly pending: number; restore(): void } {
  const queue = new Map<number, FrameRequestCallback>();
  let next = 0;
  const real = { request: globalThis.requestAnimationFrame, cancel: globalThis.cancelAnimationFrame };
  globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
    queue.set(++next, cb);
    return next;
  };
  globalThis.cancelAnimationFrame = (handle: number) => void queue.delete(handle);
  return {
    flush() {
      const due = [...queue.values()];
      queue.clear();
      for (const cb of due) cb(performance.now());
    },
    get pending() {
      return queue.size;
    },
    restore() {
      globalThis.requestAnimationFrame = real.request;
      globalThis.cancelAnimationFrame = real.cancel;
    },
  };
}

/** Calls `report` with an element's height (whole CSS pixels, rounded up) now and whenever it changes; returns the stop function. */
export function observeHeight(el: Element, report: (height: number) => void): () => void {
  if (typeof ResizeObserver === 'undefined') return () => undefined;
  let last = 0;
  const check = () => {
    const h = Math.ceil(el.getBoundingClientRect().height);
    if (h > 0 && h !== last) {
      last = h;
      report(h);
    }
  };
  const observer = new ResizeObserver(check);
  observer.observe(el);
  return () => observer.disconnect();
}

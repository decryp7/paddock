// A DevTools page handle dies when the app replaces its window (a helper turn-on, a restart of the app). `retryOnClosed` runs `fn(handle)`, and
// when it fails because the page, context or browser was closed, asks `reconnect()` for a fresh handle and runs it once more; any other error passes through.
export async function retryOnClosed(fn, handle, reconnect, { retries = 1 } = {}) {
  let h = handle;
  for (let attempt = 0; ; attempt++) {
    try { return { value: await fn(h), handle: h }; } catch (e) {
      if (!/closed/i.test(String(e?.message ?? e)) || attempt >= retries) throw e;
      h = await reconnect(h);
    }
  }
}

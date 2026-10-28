// ADR 0026 1.1-0 probe page. It reports every signal it sees through the `probe` command, and `ready` with the
// content's natural height by the trial's method:
//   a: content, fonts.ready, then the next animation frame (today's shell: afterNextRender + fonts.ready), then measure;
//   b: as a, in a window that is visible but off-screen;
//   c: content, fonts.ready, then a synchronous layout read (getBoundingClientRect), no frame wait; if that read started
//      a font load (status "loading"), wait for fonts.ready once more and read again.
(() => {
  const q = new URLSearchParams(location.search);
  const method = q.get('method') || 'a';
  const scenario = q.get('scenario') || 'normal';
  const T = window.__TAURI__;
  const label = T.window.getCurrentWindow().label;
  const send = (ev, data = {}) =>
    T.core.invoke('probe', { ev, data: { ...data, pnow: performance.now(), vis: document.visibilityState } }).catch(() => {});

  const windows = navigator.userAgent.includes('Windows');
  const fontDelay = scenario === 'font_slow' ? 400 : 0;
  const base = windows ? 'http://slow.localhost/' : 'slow://localhost/';
  const style = document.createElement('style');
  style.textContent = `@font-face { font-family: ProbeSans; src: url(${base}font?d=${fontDelay}); }`;
  document.head.appendChild(style);

  send('script', { iw: innerWidth, ih: innerHeight, dpr: devicePixelRatio, ua: navigator.userAgent, fstatus: document.fonts.status });

  const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r()));
  let raf = 0;
  const loop = () => { raf += 1; send('raf' + raf); if (raf < 2) requestAnimationFrame(loop); };
  requestAnimationFrame(loop);
  const t0 = performance.now();
  setTimeout(() => send('timeout0', { lat: performance.now() - t0 }), 0);
  setTimeout(() => send('timeout100', { lat: performance.now() - t0 }), 100);
  document.addEventListener('visibilitychange', () => send('visibility'));
  document.fonts.addEventListener('loadingdone', () => send('fonts_loadingdone', { loaded: document.fonts.check('15px ProbeSans') }));
  document.fonts.ready.then(() => send('fonts_ready_initial', { fstatus: document.fonts.status, loaded: document.fonts.check('15px ProbeSans') }));

  const text = [
    'Builds from this channel stop arriving. Machines that follow it stay on the build they have and switch to the stable channel at their next check.',
    'The channel keeps its history: you can add it again later with the same name and its past builds return.',
    'Two machines follow this channel now: build-runner-02 and lab-mac-07.',
  ];
  const content = new Promise((resolve) => {
    const fill = () => {
      const body = document.getElementById('body');
      for (const t of text) { const p = document.createElement('p'); p.textContent = t; body.appendChild(p); }
      resolve();
    };
    const start = () => (scenario === 'route_slow' ? setTimeout(fill, 600) : fill());
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
  });

  const measure = () => {
    const frame = document.getElementById('frame');
    const r = frame.getBoundingClientRect();
    const head = document.getElementById('head').getBoundingClientRect().height;
    const foot = document.getElementById('foot').getBoundingClientRect().height;
    return {
      h: r.height, w: r.width, ih: innerHeight, iw: innerWidth, oh: outerHeight, dpr: devicePixelRatio,
      minH: head + foot + 2 * 15 * 1.5,
      font: document.fonts.check('15px ProbeSans'), fstatus: document.fonts.status,
    };
  };

  let ros = 0;
  document.addEventListener('DOMContentLoaded', () => {
    send('dom');
    new ResizeObserver((entries) => {
      for (const e of entries) { ros += 1; if (ros <= 12) send('ro', { h: e.contentRect.height, w: e.contentRect.width }); }
    }).observe(document.getElementById('frame'));
  });

  (async () => {
    await content;
    await document.fonts.ready;
    let m;
    if (method === 'c') {
      m = measure();
      if (document.fonts.status === 'loading') { await document.fonts.ready; m = { ...measure(), refont: true }; }
    } else {
      await nextFrame();
      m = measure();
    }
    send('ready', m);
  })();

  T.event.listen('probe-shown', async (e) => {
    if (e.payload !== label) return;
    send('shown_sync', measure());
    await nextFrame();
    send('after_show', measure());
    setTimeout(() => send('settle', measure()), 300);
  });
})();

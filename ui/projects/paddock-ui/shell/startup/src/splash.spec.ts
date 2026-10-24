import { splashProblems } from './splash';

const good = `<!doctype html><html><head><title>x</title></head><body>
<app-root><div class="pd-splash" role="status" data-testid="boot-splash"><div class="pd-splash__logo"></div><span>Starting Pitwall&hellip;</span></div></app-root>
<script src="main.js" type="module"></script></body></html>`;

describe('splashProblems', () => {
  it('accepts a page with the splash in its root and no inline style or script', () => {
    expect(splashProblems(good, 'app-root')).toEqual([]);
  });

  it('names each problem', () => {
    expect(splashProblems(good, 'sd-root')).toEqual(['no <sd-root> root element']);
    expect(splashProblems('<app-root></app-root>', 'app-root')).toEqual(['<app-root> has no .pd-splash splash']);
    expect(splashProblems('<app-root><div class="pd-splash"><span>Loading</span></div></app-root>', 'app-root')).toEqual([
      'the splash needs role="status"',
      'the splash needs a "Starting …" text',
    ]);
    const inline = good.replace('<title>x</title>', '<style>p{}</style><script>boot()</script>').replace('<span>', '<span style="color:red">');
    expect(splashProblems(inline, 'app-root')).toEqual(['inline styles break the CSP', 'inline scripts break the CSP']);
  });
});

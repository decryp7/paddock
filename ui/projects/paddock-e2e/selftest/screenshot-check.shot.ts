import { expect, test } from '@playwright/test';
import { captureChecked, screenshotProblems } from '../src/index.ts';

// The screenshot check on small fixture pages: a clean page passes, and each kind of defect is reported with what and where.
const page = (body: string) => `data:text/html,${encodeURIComponent(`<!doctype html><style>body{margin:0;font:16px sans-serif}.box{width:60px;overflow:hidden;white-space:nowrap}.ell{width:60px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}.pd-pill{display:inline-block;min-width:8px;min-height:8px;background:green}</style>${body}`)}`;

test('a clean page has no problems', async ({ page: p }) => {
  await p.goto(page('<h1>Overview</h1><p>Read only. iPhone and macOS are fine.</p><span class="pd-pill">Healthy</span><span class="pd-pill"><svg width="8" height="8"></svg></span><code>READ_ONLY</code><div class="ell" title="A long description of the thing">A long description of the thing</div>'));
  expect(await screenshotProblems(p)).toEqual([]);
});

test('text cut by its box without a declared ellipsis is reported; a declared ellipsis is not', async ({ page: p }) => {
  await p.goto(page('<div class="box">A sentence far too long for its box</div>'));
  expect((await screenshotProblems(p)).join('\n')).toMatch(/needs \d+px, has 60px|cut by its box/);
  await p.goto(page('<div class="ell">A sentence far too long for its box</div>'));
  expect((await screenshotProblems(p)).filter((x) => /needs|cut/.test(x))).not.toHaveLength(0); // an ellipsis without a full-text tip is not accepted
  await p.goto(page('<div class="ell" data-full-text="A sentence far too long for its box">A sentence far too long for its box</div>'));
  expect(await screenshotProblems(p)).toEqual([]);
});

test('raw identifiers, placeholders and debug strings are reported', async ({ page: p }) => {
  await p.goto(page('<p>IN_PROGRESS</p><p>pdfViewer</p><p>Size: NaN</p><p>[object Object]</p><p>Lorem ipsum</p><p>TODO: later</p><p>Mock data</p><p hidden>HIDDEN_ONE</p><pre>RAW_IN_PRE</pre>'));
  const problems = (await screenshotProblems(p)).join('\n');
  for (const s of ['IN_PROGRESS', 'pdfViewer', 'NaN', 'object Object', 'Lorem', 'TODO', 'Mock']) expect(problems).toContain(s);
  expect(problems).not.toContain('HIDDEN_ONE');
  expect(problems).not.toContain('RAW_IN_PRE');
});

test('a status chip with neither word nor icon is reported', async ({ page: p }) => {
  await p.goto(page('<span class="pd-pill"></span><pd-status-pill>Fine</pd-status-pill>'));
  const problems = await screenshotProblems(p);
  expect(problems).toHaveLength(1);
  expect(problems[0]).toMatch(/status chip without a word or icon/);
});

test('allow and ignore options skip what is meant', async ({ page: p }) => {
  await p.goto(page('<p>READ_ONLY</p><p class="raw">pdfViewer</p>'));
  expect(await screenshotProblems(p, { allow: ['READ_ONLY'], ignore: '.raw' })).toEqual([]);
});

test('captureChecked refuses to photograph a defect', async ({ page: p }) => {
  await p.goto(page('<p>SOME_ENUM</p>'));
  await expect(captureChecked(p, '/nonexistent-never-written/shot.png')).rejects.toThrow(/SOME_ENUM/);
});

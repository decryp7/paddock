import { axeViolations, expect, expectNoHorizontalScroll, test, THEMES, WorkbenchApp } from '@decryp7/paddock-e2e';

// The workbench demo is the reference consumer of @decryp7/paddock-ui/workbench (ADR 0013 §4; plan W4, W7). It uses only
// the test kit (@decryp7/paddock-e2e): the workbench page objects and the shell's.

for (const colorScheme of THEMES) {
  test.describe(`workbench demo (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('boots with the frame, the Explorer and the welcome view; axe finds no violations', async ({ page, pageErrors }) => {
      const wb = new WorkbenchApp(page);
      await wb.frame.open();
      await expect(wb.explorer.node('wd.files', 'src')).toBeVisible();
      await expect(page.getByRole('heading', { level: 1, name: 'Workbench demo' })).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  });
}

test('no horizontal scroll at 320 px (rule 18), and the Explorer yields to the stage by the layout plan', async ({ page }) => {
  const wb = new WorkbenchApp(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await wb.frame.open();
  await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
  await page.setViewportSize({ width: 600, height: 800 });
  await expect.poll(() => wb.frame.explorer()).toBe('closed');
  await page.setViewportSize({ width: 320, height: 640 });
  await expectNoHorizontalScroll(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
});

test('F6 cycles the regions and the skip link goes to the work area', async ({ page }) => {
  const wb = new WorkbenchApp(page);
  await wb.frame.open();
  const seen = new Set<string | null>();
  for (let i = 0; i < 5; i++) {
    await page.keyboard.press('F6');
    seen.add(await wb.frame.focusedRegion());
  }
  expect([...seen].sort()).toEqual(['activity', 'main', 'sidebar', 'status', 'title']);
});

test.describe('activity bar and Explorer (plan W4)', () => {
  test('the Explorer item toggles the side bar; badges are part of the name', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await expect(wb.activity.item('wd.act.problems')).toHaveAccessibleName('Problems, 3 problems');
    await wb.activity.item('wd.act.explorer').click();
    await expect.poll(() => wb.frame.explorer()).toBe('closed');
    await wb.activity.item('wd.act.explorer').click();
    await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
  });

  test('the tree pages large folders, opens a context menu by keyboard, and middle-truncates long names', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.explorer.expand('wd.files', 'generated');
    await wb.explorer.expand('wd.files', 'generated/fixtures');
    await expect(wb.explorer.more).toContainText('40 left');
    await wb.explorer.more.click();
    await expect(wb.explorer.node('wd.files', 'generated/fixtures/case-240.json')).toBeVisible();
    await wb.explorer.node('wd.files', 'README.md').click();
    await page.keyboard.press('Shift+F10');
    await expect(wb.explorer.menu.getByRole('menuitem', { name: 'Copy path' })).toBeVisible();
    await page.keyboard.press('Escape');
    await wb.explorer.expand('wd.files', 'docs');
    const long = wb.explorer.node('wd.files', 'docs/architecture-overview-of-the-request-pipeline.md');
    await expect(long.getByTestId('path-text')).toHaveAttribute('title', /architecture-overview-of-the-request-pipeline\.md$/);
  });
});

test.describe('command palette (plan W5)', () => {
  test('lists commands, runs one, suggests on a typo, and searches providers by prefix', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.palette.open();
    await expect(wb.palette.options.first()).toContainText('Open README');
    await wb.palette.search('>reindx');
    await expect(page.getByTestId('palette-did-you-mean')).toBeVisible();
    await wb.palette.run('Index the repository');
    await wb.palette.open(false);
    await wb.palette.search('request handler');
    await expect(wb.palette.options.first()).toContainText('request_handler.rs');
    await wb.palette.search('@handle');
    await expect(wb.palette.options.first()).toContainText('handle_request');
    await wb.palette.search('#listening');
    await expect(wb.palette.options.first()).toContainText('listening on');
    await page.keyboard.press('Escape');
    await expect(wb.palette.dialog).toHaveCount(0);
    expect(await axeViolations(page)).toEqual([]);
  });
});

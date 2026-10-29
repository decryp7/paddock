import { dialogWindowUrl, expect, test } from '@decryp7/paddock-e2e';

/** The alert-rule dialog's rows: the controls of a row share top and bottom, all as tall as an outline field (a field's height). */
test('the alert-rule dialog lays its controls out on one grid, each row aligned and one field tall', async ({ page }) => {
  await page.setViewportSize({ width: 560, height: 700 });
  await page.goto(dialogWindowUrl('dy.alert-rule', { rule: null, takenIds: [] }, { title: 'Add rule' }));
  await page.locator('html[data-dialog-ready]').waitFor();
  const box = async (selector: string) => (await page.locator(selector).first().boundingBox())!;
  const limit = await box('pd-field:has(input[aria-label="Limit"]) mat-form-field');
  const op = await box('.seg-field >> nth=0');
  const hold = await box('pd-field:has(input[aria-label="Duration"]) mat-form-field .mat-mdc-text-field-wrapper');
  const unit = await box('.unit-seg');
  const sev = await box('.seg-field >> nth=1');
  const left = await box('pd-field:has(input[aria-label="Name"])');
  // One left edge for the first column, one for the second.
  expect(Math.abs(op.x - left.x), 'Alarm when it is starts at the dialog\'s left edge').toBeLessThanOrEqual(6);
  expect(Math.abs(sev.x - limit.x), 'Severity and Limit share a left edge').toBeLessThanOrEqual(6);
  // Rows: tops and heights.
  expect(Math.abs(op.y - limit.y), 'row 1 tops').toBeLessThanOrEqual(2);
  expect(Math.abs(sev.y - hold.y), 'row 2 tops').toBeLessThanOrEqual(2);
  const h = Math.round(limit.height);
  for (const [name, b] of Object.entries({ op, sev, hold, unit })) expect(Math.abs(Math.round(b.height) - h), `${name} is as tall as a field (${h} px)`).toBeLessThanOrEqual(1);
  // The unit toggle sits in the same row as its field, on its centre line.
  expect(Math.abs(unit.y + unit.height / 2 - (hold.y + hold.height / 2)), 'unit toggle centred on the duration field').toBeLessThanOrEqual(3);
  expect(unit.x).toBeGreaterThan(hold.x + hold.width - 1);
});

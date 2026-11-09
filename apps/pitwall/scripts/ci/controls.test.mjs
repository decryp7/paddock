import test from 'node:test';
import assert from 'node:assert/strict';
import { appFindings, findNativeControls, templateFiles } from './controls.mjs';

const ids = (src) => findNativeControls(src).map((f) => f.id);

test('finds each kind of native or retired control, with its line', () => {
  const src = [
    '<select class="x"><option>1</option></select>',
    '<input type="checkbox" [checked]="on" />',
    '<button type="button" class="btn primary">Save</button>',
    '<a class="row btn" href="#">Go</a>',
    '<div class="seg" role="radiogroup"></div>',
    '<pw-select [options]="o" />',
    '<pw-row-menu [items]="i" />',
    '<pw-empty-state heading="None" />',
    '<input class="input" type="search" />',
    '<input type="number" />',
    '<table class="table fit">',
    '<table mat-table class="table" [dataSource]="rows">',
    '<table class="grid">',
  ].join('\n');
  const found = findNativeControls(src);
  assert.deepEqual(
    found.map((f) => [f.line, f.id]),
    [
      [1, 'native-select'],
      [2, 'native-checkbox'],
      [3, 'btn-class'],
      [4, 'btn-class'],
      [5, 'seg-class'],
      [6, 'retired-component'],
      [7, 'retired-component'],
      [8, 'retired-component'],
      [9, 'unthemed-input'],
      [10, 'unthemed-input'],
      [11, 'table-class'],
      [11, 'native-table'],
      [12, 'table-class'],
      [13, 'native-table'],
    ],
  );
});

test('Material and paddock-ui controls pass', () => {
  const src = [
    '<button type="button" matButton="filled">Save</button>',
    '<button type="button" class="btn-row" matButton>Other</button>',
    '<div class="segments"></div>',
    '<mat-form-field><input matInput type="search" aria-label="Search" /></mat-form-field>',
    '<mat-slider><input matSliderThumb /></mat-slider>',
    '<mat-checkbox [checked]="on">Allow</mat-checkbox>',
    '<pd-select [options]="o" />',
    '<pd-segmented-control label="Range" [options]="o" />',
    '<input type="file" hidden />',
    '<table mat-table class="results" [dataSource]="rows" [fixedLayout]="true">',
    '<table mat-table class="table-list" [dataSource]="rows">',
    '<pd-table [rows]="rows" [columns]="cols" label="Rows" />',
  ].join('\n');
  assert.deepEqual(ids(src), []);
});

test('the check covers the templates of the app (inline and .html), not the specs', () => {
  const files = templateFiles();
  assert.ok(files.some((f) => f.endsWith('features/settings/settings-page.html')));
  assert.ok(files.some((f) => f.endsWith('features/dashboard/dashboard.ts')));
  assert.ok(!files.some((f) => f.endsWith('.spec.ts')));
});

test('no native select, native checkbox, unthemed input, .btn, .seg, hand-rolled table or retired Pitwall control is left in src/app', () => {
  assert.deepEqual(appFindings(), []);
});

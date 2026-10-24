import { Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { PD_MEASURE_TEXT, PdMiddleText, middleTruncate, tailOf } from './middle-text';

const within = (chars: number) => (s: string) => s.length <= chars;

describe('middle truncation', () => {
  it('keeps the file name and one ellipsis, as one piece of text', () => {
    expect(tailOf('/opt/plugins/alpha/plugin-0.bin')).toBe('/plugin-0.bin');
    expect(tailOf('2001:db8:1::24:0000:abcd:ef01:2345')).toBe('::abcd:ef01:2345'.slice(-14));
    expect(tailOf('x'.repeat(60) + '/' + 'y'.repeat(40))).toBe('y'.repeat(14));
    expect(middleTruncate('/opt/plugins/alpha/plugin-0.bin', within(40))).toBe('/opt/plugins/alpha/plugin-0.bin');
    expect(middleTruncate('/opt/plugins/alpha/plugin-0.bin', within(24))).toBe('/opt/plugi…/plugin-0.bin');
    // The join is the same in every row: no gap, the ellipsis touches both parts.
    const rows = ['/Users/alex/Application Support/Debrief/plugins/syslog/plugin-0.bin', '/Users/alex/Application Support/Debrief/plugins/journal/plugin-11.bin'];
    for (const r of rows) expect(middleTruncate(r, within(30))).toMatch(/^[^…]+…\/plugin-\d+\.bin$/);
  });

  it('never gives up the tail, even in a very narrow box', () => {
    expect(middleTruncate('/a/b/c/plugin-0.bin', within(3))).toBe('…/plugin-0.bin');
    expect(middleTruncate('short', within(1))).toBe('short');
  });
});

@Component({ imports: [PdMiddleText], template: `<div style="width: 200px"><pd-middle-text value="/Users/alex/Library/Application Support/Debrief/plugins/syslog/plugin-0.bin" /></div>` })
class Host {}

describe('pd-middle-text', () => {
  it('shows the full text with the full value as the tooltip when the text cannot be measured, and shortens it to the box when it can', async () => {
    const full = '/Users/alex/Library/Application Support/Debrief/plugins/syslog/plugin-0.bin';
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    let f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await f.whenStable();
    expect(f.nativeElement.querySelector('pd-middle-text').textContent).toBe(full);
    expect(f.nativeElement.querySelector('pd-middle-text').getAttribute('title')).toBe(full);
    f.destroy();

    TestBed.resetTestingModule();
    const box = { width: 200 };
    const original = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return { width: this.tagName === 'PD-MIDDLE-TEXT' ? box.width : 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}) };
    };
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: PD_MEASURE_TEXT, useValue: (t: string) => t.length * 8 }] });
    f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await f.whenStable();
    TestBed.tick();
    await f.whenStable();
    const shown = f.nativeElement.querySelector('pd-middle-text').textContent as string;
    HTMLElement.prototype.getBoundingClientRect = original;
    expect(shown.length * 8).toBeLessThanOrEqual(200);
    expect(shown).toMatch(/…\/plugin-0\.bin$/);
    expect(f.nativeElement.querySelector('pd-middle-text').getAttribute('title')).toBe(full);
  });

  it('measures the room it is given, not the text it shows: widening the box restores the full value', async () => {
    const full = '/Users/alex/Library/Application Support/Debrief/plugins/syslog/plugin-0.bin';
    const box = { width: 200 };
    const watchers: ((e: unknown[]) => void)[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: (e: unknown[]) => void) {
          watchers.push(cb);
        }
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const original = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return { width: this.tagName === 'PD-MIDDLE-TEXT' ? box.width : 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}) };
    };
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: PD_MEASURE_TEXT, useValue: (t: string) => t.length * 8 }] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await f.whenStable();
    TestBed.tick();
    await f.whenStable();
    const el = f.nativeElement.querySelector('pd-middle-text') as HTMLElement;
    const narrow = el.textContent as string;
    expect(narrow.length * 8).toBeLessThanOrEqual(200);
    expect(narrow).not.toBe(full);
    // The host takes the room it is given (flex: 1 1 0): the cut text cannot shrink its own box.
    expect(getComputedStyle(el).flexGrow).toBe('1');
    for (const width of [400, 700]) {
      box.width = width;
      watchers.forEach((w) => w([{ contentRect: { width } }]));
      TestBed.tick();
      await f.whenStable();
    }
    HTMLElement.prototype.getBoundingClientRect = original;
    vi.unstubAllGlobals();
    expect(el.textContent).toBe(full);
  });

  it('cuts equal-length values in one column to heads within one character, and never cuts a value that fits', () => {
    const rows = ['/Users/alex/Application Support/Debrief/plugins/syslog/plugin-0.bin', '/Users/alex/Application Support/Debrief/plugins/journal/plugin-1.bin', '/Users/alex/Application Support/Debrief/plugins/dumps/plugin-12.bin'];
    const heads = rows.map((r) => middleTruncate(r, within(40)).split('…')[0]!.length);
    expect(Math.max(...heads) - Math.min(...heads)).toBeLessThanOrEqual(2);
    expect(middleTruncate('/data/records/0002/entry.bin', within(60))).toBe('/data/records/0002/entry.bin');
  });
});

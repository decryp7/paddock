import { Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { PD_MEASURE_TEXT, PdMiddleText, middleTruncate, tailOf } from './middle-text';

const within = (chars: number) => (s: string) => s.length <= chars;

describe('middle truncation', () => {
  it('cuts a path at segment boundaries and keeps the trailing segments that tell rows apart', () => {
    const path = '/Users/alex/Library/Application Support/Debrief/plugins/viewer/plugin-1.bin';
    expect(middleTruncate(path, within(100))).toBe(path);
    expect(middleTruncate(path, within(40))).toBe('…/Debrief/plugins/viewer/plugin-1.bin');
    expect(middleTruncate(path, within(30))).toBe('…/plugins/viewer/plugin-1.bin');
    expect(middleTruncate(path, within(21))).toBe('…/viewer/plugin-1.bin');
    // Rows that share a long head are told apart by what is kept.
    const kinds = ['viewer', 'format', 'exporter'].map((k) => middleTruncate(`/Users/alex/Library/Application Support/Debrief/plugins/${k}/plugin-1.bin`, within(40)));
    expect(new Set(kinds).size).toBe(3);
    expect(middleTruncate('C:\\Users\\alex\\Documents\\Reports\\q3.xlsx', within(28))).toBe('…\\Documents\\Reports\\q3.xlsx');
  });

  it('cuts an id, a hash or a name by characters around the kept tail, and a path whose last segment alone is too wide', () => {
    expect(tailOf('/opt/plugins/alpha/plugin-0.bin')).toBe('/plugin-0.bin');
    expect(tailOf('2001:db8:1::24:0000:abcd:ef01:2345')).toBe('::abcd:ef01:2345'.slice(-14));
    expect(tailOf('x'.repeat(60) + '/' + 'y'.repeat(40))).toBe('y'.repeat(14));
    // The kept tail is never given up, even when it alone is wider than the box: the CSS ellipsis takes over from there.
    expect(middleTruncate('abcdef0123456789abcdef0123456789', within(12))).toBe('…cdef0123456789');
    expect(middleTruncate('abcdef0123456789abcdef0123456789', within(20))).toBe('abcde…cdef0123456789');
    expect(middleTruncate('a/b', within(1))).toBe('…/b');
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
    expect(shown).toMatch(/^…\/.*plugin-0\.bin$/);
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

  it('never cuts a value that fits', () => {
    expect(middleTruncate('/data/records/0002/entry.bin', within(60))).toBe('/data/records/0002/entry.bin');
  });
});

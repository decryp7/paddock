import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { ExtractionPreview } from '../../core/api.types';
import { ExtractionPanel, groupedNumber } from './extraction-panel';
import { settle, text } from '../../../testing/test-helpers';

const art = (name: string, variant = '') => ({ name, location: '/d/' + name, kind: name.split('.').pop() ?? '', variant, size: 2048, modifiedMs: 1, createdMs: null, checksumFile: variant ? name + '.sha256' : null });

const preview = (over: Partial<ExtractionPreview> = {}): ExtractionPreview => ({
  rows: [
    {
      text: 'Acme-PRO-R3.10.00T32.iso',
      fileName: 'Acme-PRO-R3.10.00T32.iso',
      size: 1,
      modifiedMs: 1,
      read: { id: 'PRO', version: 'R3.10.00T32', channel: '', variant: '', prerelease: false, fields: {}, spans: [{ group: 'id', start: 5, end: 8 }, { group: 'major', start: 10, end: 11 }] },
      ignored: null,
    },
    { text: 'readme.txt', fileName: 'readme.txt', size: 1, modifiedMs: 1, read: null, ignored: 'ignored: no match: the pattern does not match this name' },
  ],
  products: [{ id: 'PRO', tracked: true, versions: [{ version: 'R3.10.00T32', latest: true, prerelease: false, artifacts: [art('a.iso'), art('a_x64.exe', 'x64')] }, { version: 'R3.9.00T40', latest: false, prerelease: true, artifacts: [art('b.iso')] }] }],
  scan: { scanned: 48213, matched: 37, folders: 1, tookMs: 2400, reused: false, settling: 0 },
  ignoredCount: 48176,
  hasId: true,
  createdAvailable: true,
  orderNote: 'Newest version first.',
  warnings: ['Copying a file changes its modified time.'],
  ...over,
});

@Component({ imports: [ExtractionPanel], template: `<sp-extraction-panel [extraction]="x()" />` })
class Host {
  readonly x = signal(preview());
}

describe('ExtractionPanel', () => {
  function make(x = preview()) {
    const f = TestBed.createComponent(Host);
    f.componentInstance.x.set(x);
    f.detectChanges();
    return f;
  }

  it('words the scan like the wizard: counts with spaces, time and matches', async () => {
    const f = make();
    await settle(f);
    expect(text(f.nativeElement.querySelector('[data-testid=scan-summary]'))).toContain('Scanned 48 213 entries in 2.4 s, 37 matched.');
    expect(groupedNumber(1234567)).toBe('1 234 567');
    expect(groupedNumber(12)).toBe('12');
  });

  it('highlights the matched parts and says why a name was ignored', async () => {
    const f = make();
    await settle(f);
    const marks = [...f.nativeElement.querySelectorAll('td mark')].map((m) => (m as HTMLElement).textContent);
    expect(marks).toEqual(['PRO', '3']);
    const t = text(f.nativeElement);
    expect(t).toContain('ignored: no match');
    expect(t).toContain('R3.10.00T32');
    expect(t).toContain('product id');
  });

  it('shows products with versions, files, variants and the latest', async () => {
    const f = make();
    await settle(f);
    const t = text(f.nativeElement);
    expect(t).toContain('PRO');
    expect(t).toContain('2 versions');
    expect(t).toContain('Latest');
    expect(t).toContain('Pre-release');
    expect(t).toContain('x64');
    expect(t).toContain('checksum');
    expect(t).toContain('Newest version first.');
    expect(t).toContain('Copying a file changes its modified time.');
    expect(f.nativeElement.querySelectorAll('.versions > li.latest')).toHaveLength(1);
  });

  it('can show only the ignored names', async () => {
    const f = make();
    await settle(f);
    const box = f.nativeElement.querySelector('input[type=checkbox]') as HTMLInputElement;
    box.click();
    await settle(f);
    expect(f.nativeElement.querySelectorAll('tbody tr')).toHaveLength(1);
    expect(text(f.nativeElement)).toContain('readme.txt');
  });

  it('copes with no scan, no products and a single watch of one thing', async () => {
    const f = make(preview({ scan: null, products: [{ id: null, tracked: true, versions: [{ version: '1.0.0', latest: true, prerelease: false, artifacts: [art('x.zip')] }] }], hasId: false, warnings: [], ignoredCount: 1, rows: [] }));
    await settle(f);
    const t = text(f.nativeElement);
    expect(t).not.toContain('Scanned');
    expect(t).toContain('Releases');
    expect(t).toContain('No names to show.');
    const none = make(preview({ products: [], rows: [{ text: 'a', fileName: 'a', size: null, modifiedMs: null, read: { id: null, version: '1', channel: 'beta', variant: 'lite', prerelease: true, fields: {}, spans: [] }, ignored: null }], scan: { scanned: 1, matched: 1, folders: 1, tookMs: 5, reused: true, settling: 2 } }));
    await settle(none);
    const t2 = text(none.nativeElement);
    expect(t2).toContain('Scanned 1 entry');
    expect(t2).toContain('previous listing was reused');
    expect(t2).toContain('2 files are still changing');
    expect(t2).toContain('lite');
  });
});

import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { RunnersPage } from './runners-page';

describe('RunnersPage', () => {
  let mock: MockBackend;
  let el: HTMLElement;
  beforeEach(async () => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    await mock.status();
    const f = TestBed.createComponent(RunnersPage);
    await settle(f, 6);
    el = f.nativeElement as HTMLElement;
  });
  afterEach(() => mock.dispose());

  const row = (name: string) => Array.from(el.querySelectorAll('tbody tr')).find((r) => text(r).includes(name)) as HTMLElement;

  it('uses sentence-case headers and calls the size column Disk', () => {
    const heads = Array.from(el.querySelectorAll('thead th')).map((h) => text(h));
    expect(heads).toEqual(['Runner', 'Status', 'Current job', 'Version', 'Up for', 'Disk', 'Actions']);
  });

  it('shows status as a dot and a word, not a pill', () => {
    expect(el.querySelector('tbody .badge:not(.update)')).toBeNull();
    expect(text(row('docker-lab').querySelector('pc-runner-status'))).toBe('Crash loop');
    expect(row('docker-lab').querySelector('pc-runner-status .dot')?.className).toContain('tone-danger');
  });

  it('keeps the sub-line to one line: kind, two labels and +N with all labels in the tooltip', () => {
    const sub = row('build-win').querySelector('.sub') as HTMLElement;
    expect(text(sub)).toBe('gitea-runner · windows, x64 +1');
    expect(sub.title).toBe('windows, x64, msvc');
  });

  it('shows leftover processes as a warning icon with a tooltip, not a pill', () => {
    const r = row('build-win');
    const icon = r.querySelector('a.leftover') as HTMLElement;
    expect(icon.title).toBe('3 leftover processes');
    expect(r.querySelector('.leftover-chip')).toBeNull();
  });

  it('shows one neutral update token beside the version', () => {
    const tokens = row('docker-lab').querySelectorAll('.badge.update');
    expect(tokens).toHaveLength(1);
    expect(text(tokens[0])).toBe('5.3.0');
  });

  it('gives a problem a tooltip with the full text and a View logs link', () => {
    const cell = row('docker-lab').querySelector('.job') as HTMLElement;
    const reason = cell.querySelector('.tone-danger') as HTMLElement;
    expect(reason.title).toBe(reason.textContent?.trim());
    expect(reason.title.length).toBeGreaterThan(0);
    const link = byLink(cell);
    expect(text(link)).toBe('View logs');
    expect(link?.getAttribute('href')).toContain('/logs');
  });

  it('offers a logs icon button and the actions menu on each row', () => {
    const r = row('build-win');
    expect(r.querySelector('.row-actions a[aria-label="Logs of build-win"]')).not.toBeNull();
    expect(r.querySelector('button[aria-label="Actions for build-win"]')).not.toBeNull();
  });
});

function byLink(root: HTMLElement): Element | null {
  return Array.from(root.querySelectorAll('a')).find((a) => text(a) === 'View logs') ?? null;
}

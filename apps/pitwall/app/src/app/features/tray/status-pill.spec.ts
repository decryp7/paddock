import { TestBed } from '@angular/core/testing';
import type { PopoverStatus } from '../../core/generated/widgets';
import { StatusPill } from './status-pill';

function pill(status: PopoverStatus | null): { text: string; tone: string | null; glyph: boolean } {
  const f = TestBed.createComponent(StatusPill);
  f.componentRef.setInput('status', status);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  const p = el.querySelector('.pill')!;
  return { text: p.textContent!.trim(), tone: p.getAttribute('data-tone'), glyph: p.querySelector('pd-status-glyph') !== null };
}

describe('the status pill', () => {
  it.each<[PopoverStatus | null, string, string]>([
    [null, 'Connecting…', 'neutral'],
    [{ kind: 'unknown' }, 'Connecting…', 'neutral'],
    [{ kind: 'serving', port: 11435 }, 'Serving on :11435', 'success'],
    [{ kind: 'paused' }, 'Paused', 'warning'],
    [{ kind: 'stopped' }, 'Stopped', 'warning'],
    [{ kind: 'problems', count: 1 }, '1 engine problem', 'danger'],
    [{ kind: 'problems', count: 3 }, '3 engine problems', 'danger'],
    [{ kind: 'unreachable' }, 'Not responding', 'danger'],
  ])('%j reads "%s" in the %s tone, always with an icon', (status, text, tone) => {
    expect(pill(status)).toEqual({ text, tone, glyph: true });
  });
});

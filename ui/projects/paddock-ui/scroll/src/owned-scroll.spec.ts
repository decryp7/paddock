import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { PdOwnedScroll } from './owned-scroll';

@Component({
  selector: 'pd-owned-scroll-host',
  imports: [PdOwnedScroll],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="list" pdOwnedScroll></div>`,
})
class Host {}

describe('pdOwnedScroll', () => {
  it('adds the pd-owned-scroll class and binds overflow-anchor: none and scroll-behavior: auto (family ADR 0105)', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    const el = (fixture.nativeElement as HTMLElement).querySelector<HTMLElement>('.list')!;
    expect(el.classList).toContain('pd-owned-scroll');
    expect(el.classList).toContain('list');
    expect(el.style.getPropertyValue('overflow-anchor')).toBe('none');
    expect(el.style.getPropertyValue('scroll-behavior')).toBe('auto');
  });
});

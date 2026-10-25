import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { ViewPrefs } from '../../core/view-prefs';
import { LiveStore } from '../../state/live.store';
import { CpuTile } from './tiles';

function setup(cpu: 'hybrid' | 'uniform') {
  localStorage.clear();
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, cpu });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  const f = TestBed.createComponent(CpuTile);
  f.detectChanges();
  return f;
}

describe('CPU tile', () => {
  it('toggles to per-core bars grouped by kind on a hybrid CPU and remembers the choice', () => {
    const f = setup('hybrid');
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.cores')).not.toBeNull();
    [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Per core')!.click();
    f.detectChanges();
    expect([...el.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['Performance cores', 'Efficiency cores']);
    expect(el.querySelectorAll('li')).toHaveLength(14);
    expect(TestBed.inject(ViewPrefs).cpuView()).toBe('cores');
    expect(el.querySelector('a[aria-label="CPU details"]')).not.toBeNull();
  });

  it('shows one plain Cores group on a uniform CPU', () => {
    const f = setup('uniform');
    TestBed.inject(ViewPrefs).setCpuView('cores');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect([...el.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['Cores']);
  });
});

import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { provideIcons } from '../../shared/icons';
import { CleanupPage } from '../cleanup/cleanup-page';
import { HealthPage } from '../health/health-page';
import { ReportsPage } from '../reports/reports-page';
import { RulesPage } from '../rules/rules-page';
import { WhySlowPage } from '../why-slow/why-slow-page';
import { PLANNED, Planned, type PlannedKey } from './planned';

function render(component: new () => unknown) {
  TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons()] });
  const f = TestBed.createComponent(component as never);
  f.detectChanges();
  return f.nativeElement as HTMLElement;
}

describe('Planned', () => {
  it.each(Object.keys(PLANNED) as PlannedKey[])('renders the %s placeholder with its text and no data', (key) => {
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons()] });
    const f = TestBed.createComponent(Planned);
    f.componentRef.setInput('page', key);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('h1')?.textContent).toBe(PLANNED[key].title);
    expect(el.textContent).toContain(PLANNED[key].text);
    expect(el.querySelector('.badge')?.textContent?.trim()).toBe('Planned');
    expect(el.textContent).toContain('not built yet');
    expect(el.querySelector('a[href="/"]')).not.toBeNull();
    // No invented numbers, tables or charts.
    expect(el.querySelector('table, canvas, svg:not(mat-icon svg)')).toBeNull();
    expect(el.textContent).not.toMatch(/\d+\s?%/);
  });

  it.each([
    ['why-slow', WhySlowPage, 'Why slow'],
    ['rules', RulesPage, 'CPU taming'],
    ['cleanup', CleanupPage, 'Cleanup'],
    ['health', HealthPage, 'Health & tuning'],
    ['reports', ReportsPage, 'Reports & alerts'],
  ] as const)('the %s route page shows its planned title', (_n, page, title) => {
    const el = render(page);
    expect(el.querySelector('dy-planned')).not.toBeNull();
    expect(el.querySelector('h1')?.textContent).toBe(title);
    expect(el.textContent).toContain('Planned');
  });
});

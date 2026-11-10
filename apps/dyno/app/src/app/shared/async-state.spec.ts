import { Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { CommandError } from '../core/api.types';
import { AsyncState } from './async-state';
import { expectNameAllowed } from '../../testing/aria-name';
import { provideIcons } from './icons';

@Component({
  imports: [AsyncState],
  template: `
    <dy-async [loaded]="loaded()" [error]="error()" [empty]="empty()" [rows]="rows()" what="widgets" emptyTitle="No widgets" emptyText="Add one." (retry)="retries = retries + 1">
      <div skeleton class="custom-skeleton">custom</div>
      <p class="content">content</p>
      <button empty-action class="act">Add</button>
    </dy-async>
  `,
})
class Host {
  readonly loaded = input(false);
  readonly error = input<CommandError | null>(null);
  readonly empty = input(false);
  readonly rows = input(3);
  retries = 0;
}

function mount(inputs: Partial<{ loaded: boolean; error: CommandError | null; empty: boolean; rows: number }> = {}) {
  TestBed.configureTestingModule({ providers: [provideIcons()] });
  const f = TestBed.createComponent(Host);
  for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  const update = (k: string, v: unknown) => {
    f.componentRef.setInput(k, v);
    f.detectChanges();
  };
  return { f, el, update };
}

describe('AsyncState', () => {
  it('shows a labelled skeleton with the requested number of bars until loaded', () => {
    const { el } = mount({ rows: 4 });
    const wrap = el.querySelector('.skeleton-wrap')!;
    expect(wrap.getAttribute('aria-busy')).toBe('true');
    expectNameAllowed(wrap);
    expect(wrap.getAttribute('aria-label')).toBe('Loading widgets');
    expect(el.querySelectorAll('.skeleton')).toHaveLength(4);
    expect(el.querySelector('.content')).toBeNull();
  });

  it('projects a custom skeleton and no bars when rows is 0', () => {
    const { el } = mount({ rows: 0 });
    expect(el.querySelector('.custom-skeleton')).not.toBeNull();
    expect(el.querySelectorAll('.skeleton')).toHaveLength(0);
  });

  it('cycles the bar widths for more bars than presets', () => {
    const { el } = mount({ rows: 6 });
    const w = [...el.querySelectorAll<HTMLElement>('.skeleton')].map((s) => s.style.width);
    expect(w).toHaveLength(6);
    expect(w[5]).toBe(w[0]);
  });

  it('shows the content once loaded', () => {
    const { el } = mount({ loaded: true });
    expect(el.querySelector('.content')?.textContent).toBe('content');
    expect(el.querySelector('.skeleton-wrap')).toBeNull();
    expect(el.querySelector('[role=alert]')).toBeNull();
  });

  it('shows an inline error with a hint before the first answer, and Retry emits', () => {
    const { f, el } = mount({ error: { code: 'timeout', message: 'Took too long' } });
    const alert = el.querySelector('.error')!;
    expect(alert.getAttribute('role')).toBe('alert');
    expect(alert.textContent).toContain('Took too long');
    expect(alert.textContent).toContain('Try again');
    expect(el.querySelector('.skeleton-wrap')).toBeNull();
    (alert.querySelector('button') as HTMLButtonElement).click();
    expect(f.componentInstance.retries).toBe(1);
  });

  it('shows no hint line for an error code without one', () => {
    const { el } = mount({ error: { code: 'internal', message: 'Odd' } });
    expect(el.querySelectorAll('.error .text span').length).toBeLessThanOrEqual(1);
  });

  it('keeps stale content under a banner when a refresh fails', () => {
    const { f, el } = mount({ loaded: true, error: { code: 'busy', message: 'Refresh failed' } });
    expect(el.querySelector('.banner')?.textContent).toContain('Refresh failed');
    expect(el.querySelector('.content')).not.toBeNull();
    expect(el.querySelector('.error')).toBeNull();
    (el.querySelector('.banner button') as HTMLButtonElement).click();
    expect(f.componentInstance.retries).toBe(1);
  });

  it('shows the empty state with its text and action instead of the content', () => {
    const { el, update } = mount({ loaded: true, empty: true });
    expect(el.querySelector('.empty strong')?.textContent).toBe('No widgets');
    expect(el.querySelector('.empty span')?.textContent).toBe('Add one.');
    expect(el.querySelector('.empty .act')).not.toBeNull();
    expect(el.querySelector('.content')).toBeNull();
    update('empty', false);
    expect(el.querySelector('.empty')).toBeNull();
    expect(el.querySelector('.content')).not.toBeNull();
  });
});

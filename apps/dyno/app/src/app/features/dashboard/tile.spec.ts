import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideIcons } from '../../shared/icons';
import { Tile } from './tile';

@Component({
  imports: [Tile],
  template: `<dy-tile title="Battery" icon="cpu" [reason]="reason()" [ready]="ready()"><span class="content">86%</span><span badge class="b">AC</span></dy-tile>`,
})
class Host {
  readonly reason = signal<string | null>(null);
  readonly ready = signal(true);
}

function make() {
  TestBed.configureTestingModule({ providers: [provideIcons()] });
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, host: f.componentInstance };
}

describe('Tile', () => {
  it('shows its title, badge and body when ready', () => {
    const { el } = make();
    const tile = el.querySelector('dy-tile')!;
    expect(tile.getAttribute('role')).toBe('group');
    expect(tile.getAttribute('aria-label')).toBe('Battery');
    expect(el.querySelector('h2')?.textContent).toBe('Battery');
    expect(el.querySelector('.content')?.textContent).toBe('86%');
    expect(el.querySelector('.b')?.textContent).toBe('AC');
    expect(el.querySelector('.na')).toBeNull();
    expect(el.querySelector('.loading')).toBeNull();
  });
  it('shows a skeleton until the first update, with no body', () => {
    const { f, el, host } = make();
    host.ready.set(false);
    f.detectChanges();
    expect(el.querySelector('.loading')?.getAttribute('aria-busy')).toBe('true');
    expect(el.querySelectorAll('.skeleton').length).toBeGreaterThan(0);
    expect(el.querySelector('.content')).toBeNull();
  });
  it('shows "Not available on this PC" with the engine reason instead of the body', () => {
    const { f, el, host } = make();
    host.reason.set('This PC has no battery.');
    f.detectChanges();
    expect(el.querySelector('.na strong')?.textContent).toBe('Not available on this PC');
    expect(el.querySelector('.na span')?.textContent).toBe('This PC has no battery.');
    expect(el.querySelector('.content')).toBeNull();
    // The reason wins over the loading skeleton too: nothing invented.
    host.ready.set(false);
    f.detectChanges();
    expect(el.querySelector('.na')).not.toBeNull();
    expect(el.querySelector('.loading')).toBeNull();
  });
});

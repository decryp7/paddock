import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ClockService } from '../../core/clock.service';
import { provideIcons } from '../icons';
import { byText, flush, text } from '../../../testing/test-helpers';
import { hueOf } from '../identity';
import { AgoShortPipe, UntilShortPipe } from './ago.pipe';
import { Avatar } from './avatar';
import { MenuButton, type MenuEntry } from './row-menu';
import { StatusDot } from './status-dot';
import { Version } from './version';

describe('Avatar', () => {
  it('shows the monogram, a stable hue and the size', () => {
    const f = TestBed.createComponent(Avatar);
    f.componentRef.setInput('name', 'Acme Installer');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(text(el)).toBe('AI');
    expect(el.getAttribute('aria-hidden')).toBe('true');
    expect(el.style.getPropertyValue('--h')).toBe(String(hueOf('Acme Installer')));
    expect(el.style.getPropertyValue('--s')).toBe('32px');
    expect(el.classList.contains('lg')).toBe(false);
    f.componentRef.setInput('size', 48);
    f.detectChanges();
    expect(el.classList.contains('lg')).toBe(true);
    expect(el.style.getPropertyValue('--s')).toBe('48px');
  });

  it('shows ? for an empty name', () => {
    const f = TestBed.createComponent(Avatar);
    f.componentRef.setInput('name', '');
    f.detectChanges();
    expect(text(f.nativeElement)).toBe('?');
  });
});

describe('Version', () => {
  it('dims the suffix only when there is one', () => {
    const f = TestBed.createComponent(Version);
    f.componentRef.setInput('value', 'R3.10.00T32');
    f.detectChanges();
    expect(text(f.nativeElement.querySelector('.ver'))).toBe('R3.10.00');
    expect(text(f.nativeElement.querySelector('.sfx'))).toBe('T32');
    f.componentRef.setInput('value', '2.1.0');
    f.detectChanges();
    expect(f.nativeElement.querySelector('.sfx')).toBeNull();
    expect(text(f.nativeElement)).toBe('2.1.0');
  });
});

describe('StatusDot', () => {
  const make = (status: { kind: string; message: string | null }, extra: object = {}, always = false) => {
    const f = TestBed.createComponent(StatusDot);
    f.componentRef.setInput('watch', { status, paused: false, checking: false, ...extra });
    f.componentRef.setInput('always', always);
    f.detectChanges();
    return f;
  };

  it('is a bare dot while everything is fine', () => {
    const f = make({ kind: 'ok', message: null });
    expect(f.nativeElement.querySelector('.dot.success')).not.toBeNull();
    expect(f.nativeElement.querySelector('.word')).toBeNull();
    expect(f.nativeElement.querySelector('.dot').getAttribute('aria-label')).toBe('Working');
  });

  it('shows the word for a problem, paused or checking watch', () => {
    const failed = make({ kind: 'failed', message: 'x' });
    expect(failed.nativeElement.querySelector('.word')).not.toBeNull();
    expect(failed.nativeElement.querySelector('.dot').getAttribute('aria-label')).toBe(text(failed.nativeElement.querySelector('.word')));
    expect(text(make({ kind: 'ok', message: null }, { paused: true }).nativeElement)).toBe('Paused');
    expect(text(make({ kind: 'ok', message: null }, { checking: true }).nativeElement)).toBe('Checking now');
  });

  it('can always show the word', () => {
    expect(text(make({ kind: 'ok', message: null }, {}, true).nativeElement)).toBe('Working');
  });
});

describe('short ago pipes', () => {
  it('follow the clock and handle unknown times', () => {
    const clock = TestBed.inject(ClockService);
    clock.set(1_000_000_000);
    const ago = TestBed.runInInjectionContext(() => new AgoShortPipe());
    const until = TestBed.runInInjectionContext(() => new UntilShortPipe());
    expect(ago.transform(1_000_000_000 - 20 * 60_000)).toBe('20 min');
    expect(ago.transform(null)).toBe('—');
    expect(until.transform(1_000_000_000 + 6 * 3_600_000)).toBe('6 h');
    expect(until.transform(1_000_000_000 + 10_000)).toBe('now');
    expect(until.transform(1_000_000_000 - 10_000)).toBe('now');
    expect(until.transform(null)).toBe('—');
    expect(until.transform(undefined)).toBe('—');
    clock.set(1_000_000_000 + 3_600_000);
    expect(ago.transform(1_000_000_000)).toBe('1 h');
  });
});

@Component({
  imports: [MenuButton],
  template: `<sp-menu-button [entries]="entries()" [small]="small" (picked)="picks.push($event)" />`,
})
class MenuHost {
  readonly entries = signal<MenuEntry[]>([
    { id: 'edit', label: 'Edit', icon: 'edit' },
    { id: 'off', label: 'Unavailable', disabled: true },
    { id: 'delete', label: 'Delete', danger: true, separated: true },
  ]);
  small = true;
  picks: string[] = [];
}

describe('MenuButton', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = '')));

  async function open() {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    const f = TestBed.createComponent(MenuHost);
    f.detectChanges();
    const btn = f.nativeElement.querySelector('button') as HTMLButtonElement;
    btn.click();
    f.detectChanges();
    await flush(10);
    f.detectChanges();
    return { f, btn };
  }

  it('is a labelled icon button', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    const f = TestBed.createComponent(MenuHost);
    f.detectChanges();
    const btn = f.nativeElement.querySelector('button') as HTMLButtonElement;
    expect(btn.getAttribute('aria-label')).toBe('More actions');
    expect(btn.classList.contains('sm')).toBe(true);
  });

  it('lists the entries with the separator and danger style, and emits the picked id', async () => {
    const { f } = await open();
    const items = Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
    expect(items.map((i) => text(i))).toEqual(['Edit', 'Unavailable', 'Delete']);
    expect(document.querySelector('.cdk-overlay-container [role=separator]')).not.toBeNull();
    expect(items[2]!.classList.contains('danger')).toBe(true);
    expect(items[1]!.getAttribute('aria-disabled')).toBe('true');
    items[1]!.click();
    expect(f.componentInstance.picks).toEqual([]);
    byText(document.body, '.cdk-overlay-container [cdkMenuItem]', 'Delete')!.click();
    expect(f.componentInstance.picks).toEqual(['delete']);
  });

  it('does not let the click reach the row behind it', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    const f = TestBed.createComponent(MenuHost);
    f.detectChanges();
    let reached = false;
    (f.nativeElement as HTMLElement).addEventListener('click', () => (reached = true));
    (f.nativeElement.querySelector('button') as HTMLButtonElement).click();
    expect(reached).toBe(false);
  });
});

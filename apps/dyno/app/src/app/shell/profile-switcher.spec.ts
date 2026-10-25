import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { RulesStore } from '../state/rules.store';
import { ProfileSwitcher } from './profile-switcher';

async function make(compact = false) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ProfileSwitcher);
  f.componentRef.setInput('compact', compact);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  const settle = async () => {
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 5));
    f.detectChanges();
  };
  return { f, mock, el, rules: TestBed.inject(RulesStore), settle, trigger: () => el.querySelector('button')! };
}
const menuItems = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role^="menuitem"]')];

describe('ProfileSwitcher', () => {
  afterEach(() => {
    document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
  });

  it('is disabled and says "loading" until the rules arrive, then names the active profile', async () => {
    const { f, el, rules, settle, trigger } = await make();
    expect(rules.loaded()).toBe(false);
    expect(trigger().disabled).toBe(true);
    expect(trigger().getAttribute('aria-label')).toBe('Profile: loading. Change profile');
    expect(el.querySelector('.name')?.textContent).toBe('Profile');
    await settle();
    expect(rules.loaded()).toBe(true);
    expect(trigger().disabled).toBe(false);
    expect(el.querySelector('.name')?.textContent).toBe(rules.activeProfile()!.name);
    expect(trigger().getAttribute('aria-label')).toContain(rules.activeProfile()!.name);
    f.destroy();
  });

  it('uses the small button in compact mode', async () => {
    const { trigger, f } = await make(true);
    expect(trigger().classList.contains('sm')).toBe(true);
    f.destroy();
  });

  it('lists the profiles in a menu and switching one calls the store', async () => {
    const { f, rules, settle, trigger } = await make();
    await settle();
    trigger().click();
    f.detectChanges();
    const items = menuItems();
    expect(items.length).toBe(rules.profiles().length + 1);
    const target = rules.profiles().find((p) => p.id !== rules.profile())!;
    const item = items.find((i) => i.textContent?.includes(target.name))!;
    expect(item.getAttribute('aria-checked')).toBe('false');
    item.click();
    await settle();
    expect(rules.profile()).toBe(target.id);
    expect(trigger().getAttribute('aria-label')).toContain(target.name);
  });

  it('pauses all rules from the menu and shows a Paused badge, then offers Resume', async () => {
    const { f, el, rules, settle, trigger } = await make();
    await settle();
    expect(el.querySelector('.badge')).toBeNull();
    trigger().click();
    f.detectChanges();
    menuItems().find((i) => i.textContent?.includes('Pause all rules'))!.click();
    await settle();
    expect(rules.paused()).toBe(true);
    expect(el.querySelector('.badge')?.textContent).toBe('Paused');
    trigger().click();
    f.detectChanges();
    expect(menuItems().some((i) => i.textContent?.includes('Resume rules'))).toBe(true);
  });
});

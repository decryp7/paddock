import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { RulesStore } from '../../state/rules.store';
import { RulesProtected } from './rules-protected';

async function render() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const store = TestBed.inject(RulesStore);
  await store.load();
  const f = TestBed.createComponent(RulesProtected);
  f.detectChanges();
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      f.detectChanges();
    }
  };
  const el = f.nativeElement as HTMLElement;
  const add = async (text: string) => {
    const input = el.querySelector<HTMLInputElement>('input')!;
    input.value = text;
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    el.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    await settle();
  };
  return { f, el, store, settle, add };
}

describe('dy-rules-protected', () => {
  it('keeps the built-in list read-only and collapsed until opened', async () => {
    const { f, el, store } = await render();
    expect(el.querySelector('[aria-label="Built-in protected apps"]')).toBeNull();
    expect(el.textContent).toContain('Never touched: system, antivirus, audio, Dyno');
    el.querySelector<HTMLButtonElement>('.lnk')!.click();
    f.detectChanges();
    expect(el.querySelectorAll('[aria-label="Built-in protected apps"] li').length).toBe(store.ruleSet()!.builtInProtected.length);
    expect(el.querySelector('[aria-label="Built-in protected apps"] button')).toBeNull();
  });

  it('adds and removes your own apps and saves each change', async () => {
    const { el, store, settle, add } = await render();
    await add('  Mine ');
    expect(store.ruleSet()!.protected).toContain('Mine');
    expect(el.querySelector('input')!.value).toBe('');
    expect(el.querySelector('[aria-label="Your protected apps"]')?.textContent).toContain('Mine');
    el.querySelector<HTMLButtonElement>('[aria-label="Remove Mine from protected"]')!.click();
    await settle();
    expect(store.ruleSet()!.protected).not.toContain('Mine');
    expect(el.querySelector('[aria-label="Your protected apps"]')?.textContent ?? '').not.toContain('Mine');
  });

  it('refuses empty and duplicate names with a message', async () => {
    const { el, store, add } = await render();
    await add('   ');
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('executable name');
    await add(store.ruleSet()!.builtInProtected[0]!.toUpperCase());
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('already protected');
    await add('xapp');
    await add('XAPP');
    expect(store.ruleSet()!.protected.filter((n) => n.toLowerCase() === 'xapp').length).toBe(1);
  });
});

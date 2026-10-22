import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ExtensionChips } from './extension-chips';
import { settle } from '../../../testing/test-helpers';

@Component({ imports: [ExtensionChips], template: `<sp-extension-chips [value]="value()" (valueChange)="value.set($event)" />` })
class Host {
  readonly value = signal('iso, tar.gz');
}

describe('ExtensionChips', () => {
  it('toggles known types, shows custom ones and removes them', async () => {
    const f = TestBed.createComponent(Host);
    await settle(f);
    const buttons = () => [...f.nativeElement.querySelectorAll('button')] as HTMLButtonElement[];
    const by = (label: string) => buttons().find((b) => b.textContent?.trim().startsWith(label))!;
    expect(by('.iso').getAttribute('aria-pressed')).toBe('true');
    expect(by('.exe').getAttribute('aria-pressed')).toBe('false');
    expect(by('.tar.gz').textContent).toContain('×');
    by('.exe').click();
    await settle(f);
    expect(f.componentInstance.value()).toBe('iso, tar.gz, exe');
    by('.iso').click();
    await settle(f);
    by('.tar.gz').click();
    await settle(f);
    expect(f.componentInstance.value()).toBe('exe');
  });
});

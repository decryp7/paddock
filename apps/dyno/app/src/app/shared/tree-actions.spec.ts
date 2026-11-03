import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideIcons } from './icons';
import { TreeActions } from './tree-actions';

@Component({
  imports: [TreeActions],
  template: `<dy-tree-actions [what]="what" [disabled]="off()" (expandAll)="log.push('expand')" (collapseAll)="log.push('collapse')" />`,
})
class Host {
  what = 'programs';
  off = signal(false);
  log: string[] = [];
}

function setup() {
  TestBed.configureTestingModule({ providers: [provideIcons()] });
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const buttons = () => [...(f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('button')];
  return { f, buttons };
}

describe('TreeActions', () => {
  it('has two icon buttons with accessible names that say what they fold, and tells the page', () => {
    const { f, buttons } = setup();
    expect(buttons().map((b) => b.getAttribute('aria-label'))).toEqual(['Expand all programs', 'Collapse all programs']);
    buttons()[0]!.click();
    buttons()[1]!.click();
    expect(f.componentInstance.log).toEqual(['expand', 'collapse']);
  });

  it('can be disabled', () => {
    const { f, buttons } = setup();
    f.componentInstance.off.set(true);
    f.detectChanges();
    buttons()[0]!.click();
    expect(buttons().every((b) => b.disabled)).toBe(true);
    expect(f.componentInstance.log).toEqual([]);
  });
});

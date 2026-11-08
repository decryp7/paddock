import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { MatTooltip } from '@angular/material/tooltip';
import { FULL_TEXT_TIP_CLASS, FullTextTip } from './full-text-tip';

@Component({ imports: [FullTextTip], template: `<span class="t" [dyFullText]="text">{{ text }}</span>` })
class Host {
  text = 'a-very-long-process-name.exe';
}

function setup(scrollWidth: number, clientWidth: number, scrollHeight = 20, clientHeight = 20) {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const de = f.debugElement.query(By.css('.t'));
  const el = de.nativeElement as HTMLElement;
  Object.defineProperty(el, 'scrollWidth', { value: scrollWidth });
  Object.defineProperty(el, 'clientWidth', { value: clientWidth });
  Object.defineProperty(el, 'scrollHeight', { value: scrollHeight });
  Object.defineProperty(el, 'clientHeight', { value: clientHeight });
  return { el, tip: de.injector.get(MatTooltip) };
}

describe('FullTextTip', () => {
  it('marks its tooltip so the pointer passes through it', () => {
    expect(setup(200, 100).tip.tooltipClass).toBe(FULL_TEXT_TIP_CLASS);
  });

  it('has no tooltip message at rest, so the text is not described twice', () => {
    expect(setup(200, 100).tip.message).toBe('');
  });

  it('offers the full text while the pointer is over a clipped element, then lets go', () => {
    const { el, tip } = setup(200, 100);
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('a-very-long-process-name.exe');
    el.dispatchEvent(new MouseEvent('mouseleave'));
    expect(tip.message).toBe('');
  });

  it('offers the full text for a line clamp that cuts a wrapped line off at the bottom', () => {
    const { el, tip } = setup(100, 100, 40, 32);
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('a-very-long-process-name.exe');
  });

  it('offers nothing for text that fits', () => {
    const { el, tip } = setup(100, 100);
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('');
  });

  it('stays shut while a menu is open', () => {
    const panel = document.body.appendChild(document.createElement('div'));
    panel.className = 'mat-mdc-menu-panel';
    const { el, tip } = setup(200, 100);
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('');
    panel.remove();
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('a-very-long-process-name.exe');
  });
});

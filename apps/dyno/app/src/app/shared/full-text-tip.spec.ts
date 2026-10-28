import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from './full-text-tip';

@Component({ imports: [FullTextTip], template: `<span class="t" [dyFullText]="text">{{ text }}</span>` })
class Host {
  text = 'a-very-long-process-name.exe';
}

function setup(scrollWidth: number, clientWidth: number) {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const de = f.debugElement.query(By.css('.t'));
  const el = de.nativeElement as HTMLElement;
  Object.defineProperty(el, 'scrollWidth', { value: scrollWidth });
  Object.defineProperty(el, 'clientWidth', { value: clientWidth });
  return { el, tip: de.injector.get(MatTooltip) };
}

describe('FullTextTip', () => {
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

  it('offers nothing for text that fits', () => {
    const { el, tip } = setup(100, 100);
    el.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.message).toBe('');
  });
});

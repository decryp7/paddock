import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { clampSash, PdSash, type SashLimits } from './sash';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSash],
  template: `<div id="pane"></div><div class="s" [pdSash]="width" [pdSashLimits]="limits" pdSashControls="pane" (pdSashStart)="log.push('start')" (pdSashMove)="width.set($event); log.push('move ' + $event)" (pdSashCommit)="log.push('commit')"></div>`,
})
class Host {
  readonly width = signal(272);
  readonly cap = signal(640);
  readonly log: string[] = [];
  readonly limits = (): SashLimits => ({ min: 240, max: this.cap() });
}

function setup() {
  const fixture = TestBed.createComponent(Host);
  fixture.detectChanges();
  const sash = fixture.nativeElement.querySelector('.s') as HTMLElement;
  const pointer = (type: string, x: number, button = 0) => {
    const e = new MouseEvent(type, { clientX: x, button, cancelable: true }) as unknown as PointerEvent;
    Object.defineProperty(e, 'pointerId', { value: 1 });
    sash.dispatchEvent(e);
    return e;
  };
  const key = (k: string, shiftKey = false) => sash.dispatchEvent(new KeyboardEvent('keydown', { key: k, shiftKey, cancelable: true }));
  return { fixture, host: fixture.componentInstance, sash, pointer, key };
}

describe('PdSash', () => {
  it('is a separator with the value, the limits and the pane it controls', () => {
    const { fixture, sash, host } = setup();
    expect(sash.getAttribute('role')).toBe('separator');
    expect(sash.getAttribute('aria-orientation')).toBe('vertical');
    expect(sash.tabIndex).toBe(0);
    expect([sash.getAttribute('aria-valuenow'), sash.getAttribute('aria-valuemin'), sash.getAttribute('aria-valuemax'), sash.getAttribute('aria-controls')]).toEqual(['272', '240', '640', 'pane']);
    host.cap.set(400);
    fixture.detectChanges();
    expect(sash.getAttribute('aria-valuemax')).toBe('400');
  });

  it('drags: start first, a move per step clamped to the limits of the gesture, one commit on release', () => {
    const { host, pointer } = setup();
    host.cap.set(300);
    pointer('pointerdown', 100);
    pointer('pointermove', 120);
    pointer('pointermove', 900);
    pointer('pointermove', -900);
    pointer('pointerup', 0);
    expect(host.log).toEqual(['start', 'move 292', 'move 300', 'move 240', 'commit']);
    pointer('pointermove', 500);
    expect(host.log).toHaveLength(5);
  });

  it('ignores a press that is not the primary button, and a second press during a drag', () => {
    const { host, pointer } = setup();
    expect(pointer('pointerdown', 100, 2).defaultPrevented).toBe(false);
    pointer('pointermove', 150);
    expect(host.log).toEqual([]);
    pointer('pointerdown', 100);
    pointer('pointerdown', 200);
    pointer('pointercancel', 0);
    expect(host.log).toEqual(['start', 'commit']);
  });

  it('keys: arrows by 16 (Shift 64), Home and End to the limits, each a start, a move and a commit; other keys are left alone', () => {
    const { host, key } = setup();
    key('ArrowRight');
    key('ArrowLeft', true);
    key('End');
    key('Home');
    expect(host.log.filter((l) => l.startsWith('move'))).toEqual(['move 288', 'move 240', 'move 640', 'move 240']);
    expect(host.log.filter((l) => l === 'commit')).toHaveLength(4);
    host.log.length = 0;
    expect(key('q')).toBe(true);
    expect(host.log).toEqual([]);
  });

  it('flips both pointer and arrows in right-to-left text', () => {
    document.documentElement.dir = 'rtl';
    try {
      const { host, pointer, key } = setup();
      pointer('pointerdown', 100);
      pointer('pointermove', 60);
      pointer('pointerup', 60);
      key('ArrowLeft');
      expect(host.log.filter((l) => l.startsWith('move'))).toEqual(['move 312', 'move 328']);
    } finally {
      document.documentElement.dir = '';
    }
  });

  it('reads the width when a gesture starts, not from the last render', () => {
    const { host, key } = setup();
    host.width.set(400);
    key('ArrowRight');
    expect(host.log).toContain('move 416');
  });

  it('a lost pointer capture ends the drag (once), whatever else was seen', () => {
    const { host, pointer, sash } = setup();
    pointer('pointerdown', 100);
    pointer('pointermove', 120);
    sash.dispatchEvent(new Event('lostpointercapture'));
    sash.dispatchEvent(new Event('lostpointercapture'));
    pointer('pointerup', 120);
    pointer('pointermove', 300);
    expect(host.log).toEqual(['start', 'move 292', 'commit']);
    pointer('pointerdown', 100);
    expect(host.log.at(-1)).toBe('start');
  });

  it('a sash destroyed mid-drag ends the drag, so the consumer is not left holding it', () => {
    const { fixture, host, pointer, sash } = setup();
    pointer('pointerdown', 100);
    pointer('pointermove', 120);
    fixture.destroy();
    expect(host.log).toEqual(['start', 'move 292', 'commit']);
    expect(() => sash.dispatchEvent(new Event('lostpointercapture'))).not.toThrow();
  });

  it('clamps inside inverted limits', () => {
    expect(clampSash(10, { min: 240, max: 100 })).toBe(240);
    expect(clampSash(999, { min: 240, max: 640 })).toBe(640);
  });
});

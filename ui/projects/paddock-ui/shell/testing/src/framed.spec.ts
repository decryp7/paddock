import { ChangeDetectionStrategy, Component, computed, provideZonelessChangeDetection, signal, TemplateRef, viewChild } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { createFramed } from './framed';

/** A multi-step content: header, footer and body all show the one `step` signal. */
@Component({
  selector: 'pd-framed-stepped',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<ng-template #head><span data-testid="stepper">{{ step() }}</span></ng-template><p data-testid="body-step">{{ step() }}</p>`,
})
class Stepped implements DialogContent {
  protected readonly step = signal(1);
  private readonly head = viewChild.required<TemplateRef<unknown>>('head');
  readonly dialogHeader = computed(() => this.head());
  readonly dialogActions = computed<readonly DialogAction[]>(() => [
    { kind: 'run', id: `next-${this.step()}`, label: `Continue ${this.step()}`, role: 'primary', run: () => this.step.update((s) => s + 1) },
  ]);
}

describe('createFramed', () => {
  it('shows the step in the header, the footer and the body in the same pass, never a frame apart', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = createFramed(Stepped);
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const seen = () => {
      const foot = root.querySelector('[data-testid^=dialog-action-next-]')!.getAttribute('data-testid')!.replace('dialog-action-next-', '');
      return { head: root.querySelector('[data-testid=stepper]')!.textContent, foot, body: root.querySelector('[data-testid=body-step]')!.textContent };
    };
    expect(seen()).toEqual({ head: '1', foot: '1', body: '1' });
    root.querySelector<HTMLElement>('[data-testid^=dialog-action-next-]')!.click();
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
      const { head, foot, body } = seen();
      expect({ head, foot, body }, `microtask ${i}`).toEqual({ head: body, foot: body, body });
    }
    await fixture.whenStable();
    expect(seen()).toEqual({ head: '2', foot: '2', body: '2' });
  });
});

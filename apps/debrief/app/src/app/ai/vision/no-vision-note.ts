import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { AiImageStore } from './ai-image.store';

/**
 * The model Pitwall serves reads text only (ADR 0070): says so and where the fix is. Debrief downloads nothing itself;
 * the model is chosen in Pitwall. Shared by the chat and the image view.
 */
@Component({
  selector: 'debrief-ai-no-vision',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="card" role="note" data-testid="ai-no-vision">
      <p class="msg" i18n="@@ai.vision.noVision.message">The model Pitwall serves reads text only, so it cannot look at images. Choose a model that reads images in Pitwall.</p>
      <div class="actions">
        <button type="button" class="btn ghost" (click)="store.dismissNoVision()" i18n="@@ai.vision.noVision.dismiss">Dismiss</button>
      </div>
    </div>
  `,
  styles: `
    .card { border: 1px solid var(--border); border-left: 3px solid var(--lvl-warn); border-radius: 6px; padding: 8px 10px; background: var(--bg-1); display: flex; flex-direction: column; gap: 6px; }
    .msg { margin: 0; }
    .actions { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  `,
})
export class AiNoVisionNote {
  protected readonly store = inject(AiImageStore);
}

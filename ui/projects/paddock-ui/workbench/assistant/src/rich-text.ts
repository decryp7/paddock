import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { type Block, IncrementalMarkdown, type Inline } from './markdown';
import type { AssistantCitation } from './wire';

/** A citation the user activated. */
export interface CitationOpened {
  readonly target: string;
  readonly title: string;
}

/**
 * Renders a model answer from the safe Markdown subset with text bindings only (no `innerHTML`, no `bypassSecurityTrust*`),
 * with evidence as buttons: a cited phrase or page emits `(cite)` and the app decides what it opens (Debrief's `AiRichText`).
 */
@Component({
  selector: 'pd-assistant-rich-text',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet],
  template: `
    <ng-template #line let-inlines>
      @for (i of inlinesOf(inlines); track $index) {
        @switch (i.kind) {
          @case ('text') {<span>{{ i.text }}</span>}
          @case ('code') {<code>{{ i.text }}</code>}
          @case ('strong') {<strong>{{ i.text }}</strong>}
          @case ('em') {<em>{{ i.text }}</em>}
          @case ('cite') {<button type="button" class="cite" data-testid="assistant-cite" [attr.title]="titleOf(i)" (click)="open(i)">{{ i.text }}</button>}
        }
      }
    </ng-template>
    @for (b of blocks(); track $index) {
      @switch (b.kind) {
        @case ('p') {<p><ng-container *ngTemplateOutlet="line; context: { $implicit: inl(b) }" /></p>}
        @case ('h') {<p class="h" role="heading" [attr.aria-level]="level(b) + 2"><ng-container *ngTemplateOutlet="line; context: { $implicit: inl(b) }" /></p>}
        @case ('ul') {<ul>@for (item of items(b); track $index) {<li><ng-container *ngTemplateOutlet="line; context: { $implicit: item }" /></li>}</ul>}
        @case ('ol') {<ol>@for (item of items(b); track $index) {<li><ng-container *ngTemplateOutlet="line; context: { $implicit: item }" /></li>}</ol>}
        @case ('code') {<pre><code>{{ codeOf(b) }}</code></pre>}
      }
    }
  `,
  styles: `
    :host { display: block; line-height: var(--pd-lh-body); overflow-wrap: anywhere; min-width: 0; }
    p { margin: 0 0 var(--pd-space-2); }
    p:last-child { margin-bottom: 0; }
    .h { font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    ul, ol { margin: 0 0 var(--pd-space-2); padding-left: var(--pd-space-6); }
    li { margin: var(--pd-space-1) 0; }
    code { font-family: var(--pd-font-mono); background: var(--pd-surface-code); padding: 0 var(--pd-space-1); border-radius: var(--pd-radius-tag); font-size: 0.92em; }
    pre { font-family: var(--pd-font-mono); background: var(--pd-surface-code); padding: var(--pd-space-2); border-radius: var(--pd-radius-control); overflow-x: auto; margin: 0 0 var(--pd-space-2); font-size: var(--pd-text-label); white-space: pre; max-width: 100%; }
    pre code { background: none; padding: 0; }
    .cite { all: unset; cursor: pointer; color: var(--pd-accent); text-decoration: underline; text-underline-offset: 2px; }
    .cite:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; border-radius: var(--pd-radius-tag); }
  `,
})
export class PdAssistantRichText {
  readonly text = input.required<string>();
  readonly citations = input<readonly AssistantCitation[]>([]);
  readonly cite = output<CitationOpened>();

  /** While an answer streams in, only its last open block is parsed again (closed blocks are kept). */
  private readonly markdown = new IncrementalMarkdown();
  protected readonly blocks = computed(() => this.markdown.parse(this.text(), this.citations()));

  protected inlinesOf(v: unknown): Inline[] {
    return v as Inline[];
  }
  protected inl(b: Block): Inline[] {
    return 'inlines' in b ? b.inlines : [];
  }
  protected level(b: Block): number {
    return b.kind === 'h' ? b.level : 1;
  }
  protected items(b: Block): Inline[][] {
    return b.kind === 'ul' || b.kind === 'ol' ? b.items : [];
  }
  protected codeOf(b: Block): string {
    return b.kind === 'code' ? b.text : '';
  }
  protected titleOf(i: Inline): string {
    return i.kind === 'cite' ? $localize`:@@pd.wb.assistant.cite.open:Open ${i.title}:title:` : '';
  }
  protected open(i: Inline): void {
    if (i.kind === 'cite') this.cite.emit({ target: i.target, title: i.title });
  }
}

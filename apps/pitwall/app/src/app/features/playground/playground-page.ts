import { ChangeDetectionStrategy, Component, ElementRef, OnInit, afterRenderEffect, computed, inject, signal, viewChildren } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { LocalModel } from '../../core/api.types';
import { bytes, duration, int, rate } from '../../shared/format';
import { Panel } from '../../shared/ui/panel';
import { PlaygroundStore, type TurnStats } from './playground.store';

const SUGGESTIONS = ['Explain what a KV cache is in two sentences.', 'Write a haiku about a pit stop.', 'Summarise: the service restarted twice after a pool timeout.'];

/** Per-answer stats in one line: `TTFT 214 ms · 61.8 tok/s · 158 tokens · cold start 2.1 s`. */
export function statsText(s: TurnStats): string {
  const parts = [`TTFT ${duration(s.ttftMs)}`, `${rate(s.genTps)} tok/s`, `${int(s.completionTokens)} tokens`];
  if (s.coldStartMs !== null) parts.push(`cold start ${duration(s.coldStartMs)}`);
  return parts.join(' · ');
}

/** Chat with a local model, or two side by side with the same prompt. */
@Component({
  selector: 'pw-playground-page',
  imports: [MatIconModule, MatTooltipModule, Panel],
  providers: [PlaygroundStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './playground-page.html',
  styleUrl: './playground-page.scss',
})
export class PlaygroundPage implements OnInit {
  protected readonly store = inject(PlaygroundStore);
  protected readonly prompt = signal('');
  protected readonly suggestions = SUGGESTIONS;
  protected readonly stats = statsText;
  protected readonly size = bytes;
  private readonly lists = viewChildren<ElementRef<HTMLElement>>('msgs');
  protected readonly byId = computed(() => new Map(this.store.models().map((m) => [m.id, m])));

  constructor() {
    // Follow the stream: keep each conversation scrolled to its newest text.
    afterRenderEffect(() => {
      this.store.panes();
      for (const l of this.lists()) l.nativeElement.scrollTop = l.nativeElement.scrollHeight;
    });
  }

  ngOnInit(): void {
    void this.store.load();
  }

  protected model(id: string | null): LocalModel | null {
    return id ? (this.byId().get(id) ?? null) : null;
  }

  protected send(text = this.prompt()): void {
    if (this.store.send(text)) this.prompt.set('');
  }

  protected onKey(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      this.send();
    }
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }
}

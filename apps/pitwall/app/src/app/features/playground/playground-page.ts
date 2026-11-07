import { ChangeDetectionStrategy, Component, ElementRef, OnInit, afterRenderEffect, computed, inject, signal, viewChildren } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { MatSlider, MatSliderThumb } from '@angular/material/slider';
import { MatTab, MatTabContent, MatTabGroup } from '@angular/material/tabs';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { LocalModel } from '../../core/api.types';
import { bytes, duration, int, rate } from '../../shared/format';
import { Panel } from '../../shared/ui/panel';
import { PlaygroundChat } from './playground-chat';
import { PlaygroundStore, type TurnStats } from './playground.store';

const SUGGESTIONS = ['Explain what a KV cache is in two sentences.', 'Write a haiku about a pit stop.', 'Summarise: the service restarted twice after a pool timeout.'];

/** Per-answer stats in one line: `TTFT 214 ms · 61.8 tok/s · 158 tokens · cold start 2.1 s`. */
export function statsText(s: TurnStats): string {
  const parts = [`TTFT ${duration(s.ttftMs)}`, `${rate(s.genTps)} tok/s`, `${int(s.completionTokens)} tokens`];
  if (s.coldStartMs !== null) parts.push(`cold start ${duration(s.coldStartMs)}`);
  return parts.join(' · ');
}

/**
 * The Playground (ADR 0023 Decision 2): two tabs. Chat is the full chat with a connected app's tools (`<pd-assistant>` over
 * the chat engine); Compare puts two models side by side on one prompt, text only and without tools, as before.
 */
@Component({
  selector: 'pw-playground-page',
  imports: [MatButton, MatFormField, MatLabel, MatInput, MatSlider, MatSliderThumb, MatIconModule, MatTooltipModule, MatTabGroup, MatTab, MatTabContent, Panel, PdSelect, PlaygroundChat],
  providers: [PlaygroundStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './playground-page.html',
  styleUrl: './playground-page.scss',
})
export class PlaygroundPage implements OnInit {
  protected readonly store = inject(PlaygroundStore);
  protected readonly prompt = signal('');
  /** The open tab: 0 Chat, 1 Compare. */
  protected readonly tab = signal(0);
  protected readonly suggestions = SUGGESTIONS;
  protected readonly stats = statsText;
  protected readonly size = bytes;
  private readonly lists = viewChildren<ElementRef<HTMLElement>>('msgs');
  protected readonly modelOptions = computed<PdSelectOption[]>(() =>
    this.store.chatModels().map((m) => ({ value: m.id, label: m.name + (m.quant ? ' · ' + m.quant : '') })),
  );
  protected readonly byId = computed(() => new Map(this.store.models().map((m) => [m.id, m])));

  constructor() {
    // Follow the stream: keep each conversation scrolled to its newest text.
    afterRenderEffect(() => {
      this.store.panes();
      for (const l of this.lists()) l.nativeElement.scrollTop = l.nativeElement.scrollHeight;
    });
  }

  ngOnInit(): void {
    // Compare always shows its two models; the one-model chat is the Chat tab now.
    this.store.setCompare(true);
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

import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { EXTRAS_STYLES } from './extras-styles';
import { MatTooltip } from '@angular/material/tooltip';

/** Which processes use the graphics processor, and how much of it and its memory. */
@Component({
  selector: 'dy-extras-gpu',
  imports: [MatTooltip, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.gpu().error" what="GPU use" [rows]="5" (retry)="store.loadGpu()">
      @if (data(); as g) {
        @if (g.unavailable; as why) {
          <div class="none"><dy-icon name="gpu" /><strong>GPU use is not available</strong><span>{{ why }}</span></div>
        } @else if (g.processes.length === 0) {
          <div class="none"><dy-icon name="gpu" /><strong>No process is using the GPU right now</strong><span>Programs that draw heavily or compute on the graphics chip appear here while they do.</span></div>
        } @else {
          <div class="tablewrap">
            <table class="table tbl" aria-label="GPU use per process">
              <thead><tr><th>Process</th><th class="r w-num">GPU load</th><th class="r w-num">GPU memory</th></tr></thead>
              <tbody>
                @for (p of g.processes; track p.pid) {
                  <tr>
                    <td><span class="ell" [matTooltip]="p.name">{{ p.name }}</span></td>
                    <td class="r">{{ percent(p.gpuPercent) }}</td>
                    <td class="r">{{ bytes(p.memoryBytes) }}</td>
                  </tr>
                }
              </tbody>
            </table>
          </div>
          @if (g.source) { <p class="faint muted">From {{ g.source }}.</p> }
        }
      }
    </dy-async>
  `,
  styles: [EXTRAS_STYLES, `.w-num { width: 150px; }`],
})
export class ExtrasGpu implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.gpu().data);
  protected readonly percent = formatPercent;
  protected readonly bytes = formatBytes;

  ngOnInit(): void {
    void this.store.ensure('gpu');
  }
}

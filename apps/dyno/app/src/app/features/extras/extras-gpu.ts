import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { GpuProcess } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { EXTRAS_STYLES } from './extras-styles';

/** Which processes use the graphics processor, and how much of it and its memory. */
@Component({
  selector: 'dy-extras-gpu',
  imports: [AsyncState, Icon, PdTable],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.gpu().error" what="GPU use" [rows]="5" (retry)="store.loadGpu()">
      @if (data(); as g) {
        @if (g.unavailable; as why) {
          <div class="none"><dy-icon name="gpu" /><strong>GPU use is not available</strong><span>{{ why }}</span></div>
        } @else if (g.processes.length === 0) {
          <div class="none"><dy-icon name="gpu" /><strong>No process is using the GPU right now</strong><span>Programs that draw heavily or compute on the graphics chip appear here while they do.</span></div>
        } @else {
          <pd-table [toolbar]="false" label="GPU use per process" [rows]="g.processes" [columns]="columns" [rowKey]="procKey" [filterable]="false" height="window" />
          @if (g.source) { <p class="faint muted">From {{ g.source }}.</p> }
        }
      }
    </dy-async>
  `,
  styles: [EXTRAS_STYLES],
})
export class ExtrasGpu implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.gpu().data);
  protected readonly percent = formatPercent;
  protected readonly bytes = formatBytes;
  protected readonly procKey = (p: GpuProcess): string => String(p.pid);
  protected readonly columns: PdColumn<GpuProcess>[] = [
    { key: 'name', header: 'Process', value: (p) => p.name, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'load', header: 'GPU load', value: (p) => p.gpuPercent, text: (p) => formatPercent(p.gpuPercent), align: 'end', sortable: false, width: '150px' },
    { key: 'memory', header: 'GPU memory', value: (p) => p.memoryBytes, text: (p) => formatBytes(p.memoryBytes), align: 'end', sortable: false, width: '150px' },
  ];

  ngOnInit(): void {
    void this.store.ensure('gpu');
  }
}

/** One handler of a chart event (`on`) or of the drawing surface (`getZr().on`). */
type Handler = (event: unknown) => void;

/**
 * An ECharts instance for jsdom, which has no canvas: it records what it was asked to do and answers the geometry questions
 * with fixed values a test may change. Structurally compatible with the `setOption`, `on`, `resize`, `dispose`,
 * `dispatchAction`, `containPixel`, `convertFromPixel` and `getZr` an app's chart wrapper calls.
 */
export class FakeChart {
  readonly options: unknown[] = [];
  readonly handlers: Record<string, Handler> = {};
  /** zrender's handlers (`getZr().on`), e.g. a right-click anywhere on the canvas. */
  readonly zrHandlers: Record<string, Handler> = {};
  readonly actions: unknown[] = [];
  resized = 0;
  disposed = false;
  /** Whether a pixel is inside the plot (default: yes). */
  contains = true;
  /** Pixel to values (default `[x * 1000, y]`). */
  convert: (pixel: readonly number[]) => number[] = ([x = 0, y = 0]) => [x * 1000, y];

  setOption(option: unknown): void {
    this.options.push(option);
  }
  /** The last option given, or undefined. */
  get lastOption(): unknown {
    return this.options.at(-1);
  }
  dispatchAction(action: unknown): void {
    this.actions.push(action);
  }
  resize(): void {
    this.resized++;
  }
  dispose(): void {
    this.disposed = true;
  }
  on(name: string, handler: Handler): void {
    this.handlers[name] = handler;
  }
  getZr(): { on(name: string, handler: Handler): void } {
    return { on: (name, handler) => void (this.zrHandlers[name] = handler) };
  }
  containPixel(_finder?: unknown, _pixel?: readonly number[]): boolean {
    return this.contains;
  }
  convertFromPixel(_finder: unknown, pixel: readonly number[]): number[] {
    return this.convert(pixel);
  }
}

/**
 * Stands in for the `echarts.init` an app's chart wrapper calls: every call returns a recording `FakeChart`. Provide
 * `fake.init` through the wrapper's init token (or loader) so the specs never load ECharts.
 */
export class FakeECharts {
  readonly instances: FakeChart[] = [];
  /** The `init` function: creates and records a chart. */
  readonly init = (): FakeChart => {
    const chart = new FakeChart();
    this.instances.push(chart);
    return chart;
  };

  /** Forgets every instance. */
  reset(): void {
    this.instances.length = 0;
  }
}

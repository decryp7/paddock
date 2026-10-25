import type { DockviewPanelApi, ITabRenderer, TabPartInitParameters } from 'dockview';
import { glyphSvg } from './dock-glyph';

/** Component name dockview uses for the tab renderer of every panel. */
export const TAB_COMPONENT = 'pd-dock-tab';

/** Draws the panel's view icon into `host` (a 16 px box); returns what tears it down. */
export type DrawTabIcon = (id: string, host: HTMLElement) => (() => void) | void;

/**
 * Tab content: a pin mark (pinned tabs), the title and a close `pd:close` glyph (14 px in a 20 px hit area). The
 * default dockview tab nests buttons inside the `role=tab` element, which assistive tech reports as nested interactive
 * controls. Here the × is decorative for the mouse; the panel menu is the tab's right-click menu (keyboard: Shift+F10 or
 * the Menu key on a focused tab) and a tab closes with Delete.
 */
export class PanelTab implements ITabRenderer {
  readonly element = document.createElement('div');
  private readonly pin = document.createElement('span');
  private readonly icon = document.createElement('span');
  private stopIcon: (() => void) | void = undefined;
  private readonly label = document.createElement('span');
  private readonly close = document.createElement('span');
  private api?: DockviewPanelApi;
  private subscription?: { dispose(): void };

  constructor(
    private readonly drawIcon: DrawTabIcon | null = null,
    private readonly closable: (id: string) => boolean = () => true,
  ) {
    this.element.className = 'pd-dock-tab';
    // The focus ring belongs to the element that carries role=tab (dockview's `.dv-tab`): focusing the label (a script, a
    // click) hands the focus to it, so no tab is ever "focused" without a visible ring.
    this.element.tabIndex = -1;
    this.element.addEventListener('focus', () => this.element.closest<HTMLElement>('[role="tab"]')?.focus());
    this.pin.className = 'pd-dock-tab-pin';
    this.pin.setAttribute('aria-hidden', 'true');
    this.pin.appendChild(pinGlyph());
    this.icon.className = 'pd-dock-tab-icon';
    this.icon.setAttribute('aria-hidden', 'true');
    this.label.className = 'pd-dock-tab-label';
    this.close.className = 'pd-dock-tab-close';
    this.close.setAttribute('aria-hidden', 'true');
    this.close.appendChild(glyphSvg('close', 14));
    this.close.title = $localize`:@@pd.wb.dock.panelTab.close:Close (Delete)`;
    this.element.append(this.pin, ...(drawIcon ? [this.icon] : []), this.label, this.close);
  }

  init(params: TabPartInitParameters): void {
    this.api = params.api;
    this.element.dataset['panelId'] = params.api.id;
    this.element.dataset['testid'] = 'pd-dock-tab';
    this.stopIcon = this.drawIcon?.(params.api.id, this.icon);
    this.label.textContent = params.title;
    this.subscription = params.api.onDidTitleChange((e) => (this.label.textContent = e.title));
    if (!this.closable(params.api.id)) this.close.remove();
    this.close.addEventListener('click', (e) => {
      e.stopPropagation();
      this.api?.close();
    });
    // Do not start a drag when pressing the close affordance.
    this.close.addEventListener('pointerdown', (e) => e.stopPropagation());
  }

  dispose(): void {
    this.subscription?.dispose();
    this.stopIcon?.();
  }
}

/** Id of the panel whose tab contains `target`, if the event came from a tab. */
export function tabPanelId(target: EventTarget | null): string | null {
  const tab = (target as HTMLElement | null)?.closest?.('[role="tab"]');
  return tab?.querySelector<HTMLElement>('.pd-dock-tab')?.dataset['panelId'] ?? null;
}

/** A 14 px pin mark for pinned tabs. */
function pinGlyph(): SVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('viewBox', '0 0 24 24');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', 'M9 4h6l-1 5.5 3 3H7l3-3zM12 12.5V20');
  path.setAttribute('fill', 'none');
  path.setAttribute('stroke', 'currentColor');
  path.setAttribute('stroke-width', '2');
  path.setAttribute('stroke-linejoin', 'round');
  svg.appendChild(path);
  return svg;
}

import { PanelTab, tabPanelId } from './panel-tab';

describe('PanelTab', () => {
  it('renders the title, follows renames and closes the panel from the x', () => {
    let rename!: (e: { title: string }) => void;
    const close = vi.fn();
    const dispose = vi.fn();
    const tab = new PanelTab();
    tab.init({ title: 'Timeline', api: { id: 'timeline', close, onDidTitleChange: (cb: typeof rename) => ((rename = cb), { dispose }) } } as never);
    expect(tab.element.textContent).toContain('Timeline');
    rename({ title: 'Renamed' });
    expect(tab.element.textContent).toContain('Renamed');
    const x = tab.element.querySelector<HTMLElement>('.pd-dock-tab-close')!;
    expect(x.getAttribute('aria-hidden')).toBe('true');
    expect(x.querySelector('svg')!.getAttribute('width')).toBe('14');
    expect(tab.element.querySelector('.pd-dock-tab-menu')).toBeNull();
    x.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    x.click();
    expect(close).toHaveBeenCalledOnce();
    tab.dispose();
    expect(dispose).toHaveBeenCalled();
    new PanelTab().dispose();
  });

  it('hands a focus on the label to the element that carries role=tab', () => {
    const carrier = document.createElement('div');
    carrier.setAttribute('role', 'tab');
    carrier.tabIndex = 0;
    const tab = new PanelTab();
    carrier.appendChild(tab.element);
    document.body.appendChild(carrier);
    tab.element.focus();
    expect(document.activeElement).toBe(carrier);
    carrier.remove();
    // Outside a tab strip nothing happens.
    document.body.appendChild(tab.element);
    expect(() => tab.element.focus()).not.toThrow();
    tab.element.remove();
  });

  it('finds the panel id of the tab an event came from', () => {
    const host = document.createElement('div');
    host.innerHTML = '<div role="tab"><div class="pd-dock-tab" data-panel-id="p1"><span id="inner"></span></div></div><div id="other"></div>';
    expect(tabPanelId(host.querySelector('#inner'))).toBe('p1');
    expect(tabPanelId(host.querySelector('#other'))).toBeNull();
    expect(tabPanelId(null)).toBeNull();
  });
});


/**
 * The dock's chrome, from `--pd-*` tokens only (light and dark follow the tokens). Applied by `pd-dock` with
 * `ViewEncapsulation.None`, every rule scoped under `pd-dock`, so a window that does not show the dock carries none of
 * it. dockview's own base stylesheet (`dockview/dist/styles/dockview.css`) is added by the app, as the demo does.
 */
export const DOCK_STYLES = `
pd-dock {
  display: grid; flex: 1; min-height: 0; min-width: 0; position: relative; width: 100%; height: 100%;
  grid-template-columns: auto minmax(0, 1fr) auto; grid-template-rows: minmax(0, 1fr) auto;
  grid-template-areas: 'left center right' 'bottom bottom bottom';
  background: var(--pd-bg); color: var(--pd-text); font-family: var(--pd-font-sans);
}
pd-dock .pd-dock-center { grid-area: center; position: relative; min-width: 0; min-height: 0; display: flex; }
pd-dock .pd-dock-mount { flex: 1; min-width: 0; min-height: 0; position: relative; }

/* Auto-hide side bars (VS): vertical labels on the sides, horizontal at the bottom. */
pd-dock .pd-dock-ah-bar { display: flex; gap: 2px; padding: 2px; background: var(--pd-bg); }
pd-dock .pd-dock-ah-left { grid-area: left; flex-direction: column; border-right: 1px solid var(--pd-hairline); }
pd-dock .pd-dock-ah-right { grid-area: right; flex-direction: column; border-left: 1px solid var(--pd-hairline); }
pd-dock .pd-dock-ah-bottom { grid-area: bottom; flex-direction: row; border-top: 1px solid var(--pd-hairline); }
pd-dock .pd-dock-ah-tab {
  border: 0; background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-caption); cursor: pointer;
  padding: 8px 4px; border-radius: var(--pd-radius-tag); white-space: nowrap; min-width: 24px; min-height: 24px;
}
pd-dock .pd-dock-ah-left .pd-dock-ah-tab, pd-dock .pd-dock-ah-right .pd-dock-ah-tab { writing-mode: vertical-rl; }
pd-dock .pd-dock-ah-left .pd-dock-ah-tab { transform: rotate(180deg); }
pd-dock .pd-dock-ah-bottom .pd-dock-ah-tab { padding: 3px 10px; }
pd-dock .pd-dock-ah-tab:hover, pd-dock .pd-dock-ah-tab.open { color: var(--pd-text); background: var(--pd-hover); box-shadow: inset 0 -2px 0 var(--pd-accent); }
pd-dock .pd-dock-ah-left .pd-dock-ah-tab:hover, pd-dock .pd-dock-ah-left .pd-dock-ah-tab.open { box-shadow: inset 2px 0 0 var(--pd-accent); }
pd-dock .pd-dock-ah-right .pd-dock-ah-tab:hover, pd-dock .pd-dock-ah-right .pd-dock-ah-tab.open { box-shadow: inset -2px 0 0 var(--pd-accent); }
pd-dock .pd-dock-ah-tab:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }

pd-dock .pd-dock-ah-flyout {
  position: absolute; z-index: 990; display: flex; flex-direction: column; background: var(--pd-surface-1);
  border: 1px solid var(--pd-border-strong); box-shadow: var(--pd-shadow-2); outline: none;
}
pd-dock .pd-dock-ah-flyout-left { left: 0; top: 0; bottom: 0; animation: pd-dock-slide-left calc(140ms * var(--pd-motion-scale, 1)) ease-out; }
pd-dock .pd-dock-ah-flyout-right { right: 0; top: 0; bottom: 0; animation: pd-dock-slide-right calc(140ms * var(--pd-motion-scale, 1)) ease-out; }
pd-dock .pd-dock-ah-flyout-bottom { left: 0; right: 0; bottom: 0; animation: pd-dock-slide-up calc(140ms * var(--pd-motion-scale, 1)) ease-out; }
@keyframes pd-dock-slide-left { from { transform: translateX(-24px); opacity: 0.4; } }
@keyframes pd-dock-slide-right { from { transform: translateX(24px); opacity: 0.4; } }
@keyframes pd-dock-slide-up { from { transform: translateY(24px); opacity: 0.4; } }
@media (prefers-reduced-motion: reduce) { pd-dock .pd-dock-ah-flyout, .pd-dock-float-cue { animation: none !important; } pd-dock .pd-dock-skeleton span { animation: none !important; } }
pd-dock .pd-dock-ah-head {
  display: flex; align-items: center; gap: 2px; height: 30px; padding: 0 4px 0 10px; flex: none;
  background: var(--pd-bg); border-bottom: 1px solid var(--pd-hairline); box-shadow: inset 0 2px 0 var(--pd-accent);
}
pd-dock .pd-dock-ah-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--pd-text-caption); color: var(--pd-text); }
pd-dock .pd-dock-ah-body { flex: 1; min-height: 0; display: flex; }
pd-dock .pd-dock-ah-body > .pd-panel { flex: 1; }
pd-dock .pd-dock-ah-grip { position: absolute; background: transparent; }
pd-dock .pd-dock-ah-flyout-left .pd-dock-ah-grip { right: -3px; top: 0; bottom: 0; width: 6px; cursor: ew-resize; }
pd-dock .pd-dock-ah-flyout-right .pd-dock-ah-grip { left: -3px; top: 0; bottom: 0; width: 6px; cursor: ew-resize; }
pd-dock .pd-dock-ah-flyout-bottom .pd-dock-ah-grip { top: -3px; left: 0; right: 0; height: 6px; cursor: ns-resize; }

/* Buttons the dock draws itself (notice, error card). */
.pd-dock-btn, pd-dock .pd-dock-btn {
  min-height: 24px; padding: 2px 10px; border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control);
  background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer;
}
.pd-dock-btn:hover { background: var(--pd-hover); }
.pd-dock-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }

/* Drop guides (VS compass and edge guides) and the preview of the drop result. */
.pd-dock-drop-guides { position: absolute; inset: 0; pointer-events: none; z-index: 1000; display: none; }
.pd-dock-drop-guides.on { display: block; }
.pd-dock-guide {
  position: absolute; display: flex; align-items: center; justify-content: center; font-size: var(--pd-text-label);
  color: var(--pd-accent); background: var(--pd-surface-1); border: 1px solid var(--pd-accent); border-radius: var(--pd-radius-tag);
  box-shadow: var(--pd-shadow-3); opacity: 0.92;
}
.pd-dock-guide-center::after { content: ''; width: 14px; height: 14px; border: 2px solid var(--pd-accent); border-radius: 2px; }
.pd-dock-guide.hot { background: var(--pd-accent-fill); color: var(--pd-on-accent); }
.pd-dock-guide-center.hot::after { border-color: var(--pd-on-accent); }
/* The cue of a tab dragged off the drop zones: a ghost of the floating panel (or new window) and the hint. */
.pd-dock-float-cue {
  position: fixed; z-index: 1001; pointer-events: none; display: flex; align-items: flex-start; justify-content: center;
  padding-top: 10px; border: 2px solid var(--pd-accent); border-radius: var(--pd-radius-popover); background: var(--pd-accent-tint);
  box-shadow: var(--pd-shadow-2); opacity: 0.6; animation: pd-dock-float-cue-in calc(120ms * var(--pd-motion-scale, 1)) ease-out;
}
.pd-dock-float-cue[hidden] { display: none; }
.pd-dock-float-cue[data-outcome='cancel'] { border: 0; background: none; box-shadow: none; padding: 0; opacity: 1; overflow: visible; justify-content: flex-start; }
.pd-dock-float-cue[data-outcome='cancel'] .pd-dock-float-cue-hint { background: var(--pd-surface-3); color: var(--pd-text); border: 1px solid var(--pd-border-strong); }
body.pd-dock-drag-cancel .dv-tab-ghost-drag { opacity: 0.5 !important; }
.dv-tab-ghost-drag { margin: 2px 0 0 42px !important; }
.pd-dock-float-cue-hint {
  padding: 4px 10px; border-radius: var(--pd-radius-tag); font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); white-space: nowrap;
  background: var(--pd-accent-fill); color: var(--pd-on-accent); box-shadow: var(--pd-shadow-3);
}
@keyframes pd-dock-float-cue-in { from { opacity: 0; } }

/* dockview mapped onto the tokens. */
.pd-dockview {
  --dv-group-view-background-color: var(--pd-surface-1);
  --dv-tabs-and-actions-container-background-color: var(--pd-surface-2);
  --dv-tabs-and-actions-container-height: 34px;
  --dv-tabs-and-actions-container-font-size: var(--pd-text-body);
  --dv-activegroup-visiblepanel-tab-background-color: var(--pd-surface-1);
  --dv-activegroup-hiddenpanel-tab-background-color: var(--pd-surface-2);
  --dv-inactivegroup-visiblepanel-tab-background-color: var(--pd-surface-1);
  --dv-inactivegroup-hiddenpanel-tab-background-color: var(--pd-surface-2);
  --dv-activegroup-visiblepanel-tab-color: var(--pd-text);
  --dv-activegroup-hiddenpanel-tab-color: var(--pd-text-muted);
  --dv-inactivegroup-visiblepanel-tab-color: var(--pd-text-muted);
  --dv-inactivegroup-hiddenpanel-tab-color: var(--pd-text-muted);
  --dv-tab-divider-color: var(--pd-hairline);
  --dv-separator-border: var(--pd-hairline);
  --dv-paneview-header-border-color: var(--pd-hairline);
  --dv-paneview-active-outline-color: var(--pd-accent);
  --dv-icon-hover-background-color: var(--pd-hover);
  --dv-drag-over-background-color: var(--pd-accent-tint);
  --dv-drag-over-border-color: var(--pd-accent);
  --dv-edge-dock-indicator-color: var(--pd-accent);
  --dv-sash-color: var(--pd-hairline);
  --dv-active-sash-color: var(--pd-accent);
  --dv-floating-box-shadow: var(--pd-shadow-2);
  --dv-floating-border: 1px solid var(--pd-border-strong);
  --dv-overlay-z-index: 999;
  --dv-tab-font-size: var(--pd-text-body);
  --dv-tabs-container-scrollbar-color: var(--pd-border-strong);
  --dv-context-menu-background-color: var(--pd-surface-3);
  --dv-context-menu-color: var(--pd-text);
  color-scheme: inherit;
  font-family: var(--pd-font-sans);
}
.pd-dockview .dv-tab { color: var(--pd-text-muted); transition: color calc(var(--pd-duration-micro, 120ms) * var(--pd-motion-scale, 1)) ease, background-color calc(var(--pd-duration-micro, 120ms) * var(--pd-motion-scale, 1)) ease; }
.pd-dockview .dv-tab:hover { color: var(--pd-text-body); }
.pd-dockview .dv-tab.dv-active-tab { color: var(--pd-text); }
.pd-dockview .dv-tab:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
.pd-dockview .dv-tabs-and-actions-container { border-bottom: 1px solid var(--pd-hairline); }
.pd-dockview .dv-groupview { background: var(--pd-surface-1); }
.pd-dockview .dv-groupview.dv-active-group .dv-tab.dv-active-tab { box-shadow: inset 0 2px 0 var(--pd-accent); background: var(--pd-surface-1); }
.pd-dockview .dv-groupview.dv-inactive-group .dv-tab.dv-active-tab { box-shadow: inset 0 2px 0 var(--pd-border-strong); }
.pd-dockview .dv-drop-target-anchor, .pd-dockview .dv-drop-target-selection { border: 2px dashed var(--pd-accent) !important; background-color: var(--pd-accent-tint) !important; border-radius: 2px; }
.dv-tabs-overflow-dropdown-default { min-width: 24px; min-height: 24px; }
.pd-dockview .dv-tabs-and-actions-container, .pd-dock-actions { -webkit-user-select: none; user-select: none; }

/* Tabs: view icon (14 px), pin mark, title, menu chevron and close, each a target of at least 16 px. */
.pd-dock-tab { display: flex; align-items: center; gap: 6px; height: 100%; padding: 0 8px 0 12px; white-space: nowrap; }
.pd-dock-tab-icon { display: inline-flex; width: 16px; height: 16px; flex: none; align-items: center; justify-content: center; }
.pd-dock-tab-icon > * { width: 16px; height: 16px; }
.pd-dock-tab-label { overflow: hidden; text-overflow: ellipsis; }
.pd-dock-tab-pin { display: none; color: var(--pd-text-subtle); }
.pd-dock-tab.pinned .pd-dock-tab-pin { display: inline-flex; }
.pd-dock-tab-menu, .pd-dock-tab-close { width: 16px; height: 16px; line-height: 15px; text-align: center; border-radius: var(--pd-radius-tag); color: var(--pd-text-subtle); visibility: hidden; flex: none; }
.pd-dock-tab-menu { font-size: var(--pd-text-label); }
.pd-dock-tab-close { font-size: var(--pd-text-body-lg); }
.dv-tab:hover .pd-dock-tab-menu, .dv-tab.dv-active-tab .pd-dock-tab-menu, .dv-tab:focus-visible .pd-dock-tab-menu,
.dv-tab:hover .pd-dock-tab-close, .dv-tab.dv-active-tab .pd-dock-tab-close, .dv-tab:focus-visible .pd-dock-tab-close { visibility: visible; }
.dv-tab:not(.dv-active-tab) .pd-dock-tab-menu { display: none; }
.pd-dock-tab-menu:hover, .pd-dock-tab-close:hover { background: var(--pd-hover); color: var(--pd-text); }

/* Group header actions keep a full 24x24 target. */
.pd-dock-actions { display: flex; align-items: center; gap: 2px; padding: 0 6px; height: 100%; }
.pd-dock-action {
  flex: none; min-width: 24px; min-height: 24px; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--pd-radius-control);
  background: transparent; color: var(--pd-text-muted); display: inline-flex; align-items: center; justify-content: center; cursor: pointer;
}
.pd-dock-action:hover { background: var(--pd-hover); color: var(--pd-text); }
.pd-dock-action:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
.pd-dock-action[hidden] { display: none; }

/* A panel: fills its group; the loading skeleton and the error card live inside it. */
.pd-panel { height: 100%; width: 100%; min-width: 0; min-height: 0; display: flex; flex-direction: column; background: var(--pd-surface-1); color: var(--pd-text); container-type: inline-size; }
.pd-panel-outlet { display: contents; }
.pd-panel-error { padding: 16px; color: var(--pd-text-muted); display: flex; flex-direction: column; align-items: flex-start; gap: 8px; }
.pd-panel-actions { display: flex; gap: 8px; flex-wrap: wrap; }
.pd-dock-skeleton { display: flex; flex-direction: column; gap: 10px; padding: 16px; }
.pd-dock-skeleton span { display: block; height: 14px; border-radius: var(--pd-radius-tag); background: linear-gradient(90deg, var(--pd-skeleton) 0%, var(--pd-skeleton-shine) 50%, var(--pd-skeleton) 100%); background-size: 200% 100%; animation: pd-dock-shine 1.4s linear infinite; }
.pd-dock-skeleton span:nth-child(1) { width: 60%; } .pd-dock-skeleton span:nth-child(2) { width: 90%; } .pd-dock-skeleton span:nth-child(3) { width: 75%; } .pd-dock-skeleton span:nth-child(4) { width: 40%; }
@keyframes pd-dock-shine { from { background-position: 100% 0; } to { background-position: -100% 0; } }

/* Popup menu (panel menu, auto-hide side). */
.pd-dock-menu-backdrop { position: fixed; inset: 0; z-index: 1990; }
.pd-dock-menu {
  position: fixed; z-index: 1991; min-width: 180px; max-width: 80vw; margin: 0; padding: 4px; list-style: none;
  background: var(--pd-surface-3); color: var(--pd-text); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-popover); box-shadow: var(--pd-shadow-3);
}
.pd-dock-menu button { display: block; width: 100%; text-align: start; min-height: 24px; padding: 4px 10px; border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: inherit; font: inherit; font-size: var(--pd-text-body); cursor: pointer; }
.pd-dock-menu button:hover:not(:disabled), .pd-dock-menu button:focus-visible { background: var(--pd-hover); outline: none; box-shadow: inset 0 0 0 2px var(--pd-focus-ring); }
.pd-dock-menu button:disabled { color: var(--pd-text-subtle); cursor: default; }

/* Full screen: the frame hides its chrome on html.pd-dock-full-screen; the dock hides its auto-hide bars. */
html.pd-dock-full-screen .pd-dock-ah-bar { display: none !important; }
`;

/** Table, dot and icon-button styles the Alarms parts share (each component bundles its own copy). */
export const TABLE_CSS = `
  .grid { container-type: inline-size; border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: clip; }
  .head, .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: var(--sp-3); padding: 0 var(--sp-3); }
  .head { height: 30px; border-bottom: 1px solid var(--border); background: var(--surface-1); color: var(--text-3); font-size: var(--fs-xs); font-weight: var(--fw-medium); position: sticky; top: 0; z-index: 3; }
  .row { min-height: 40px; border-bottom: 1px solid var(--border-subtle); font-size: var(--fs-sm); }
  .row:last-child { border-bottom: 0; }
  .row:hover, .row:focus-visible { background: var(--surface-hover); outline: none; }
  .row:focus-visible { box-shadow: inset 0 0 0 2px var(--accent); }
  .cell { min-width: 0; display: flex; align-items: center; gap: var(--sp-2); white-space: nowrap; }
  .r { justify-content: flex-end; text-align: right; }
  .muted { color: var(--text-2); }
  .faint { color: var(--text-3); }
  .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; background: var(--text-3); }
  .dot[data-tone='bad'] { background: var(--bad); }
  .dot[data-tone='warn'] { background: var(--warn); }
  .dot[data-tone='info'] { background: var(--info); }
  .dot[data-tone='ok'] { background: var(--ok); }
  .dot[data-tone='calm'] { background: transparent; box-shadow: inset 0 0 0 1.5px var(--text-3); }
  .id { font: var(--fw-regular) var(--fs-sm) / 1.2 var(--font-mono); }
  .acts { justify-content: flex-end; gap: 2px; }
  .acts .ib { opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
  .acts .more { opacity: 0.4; }
  .row:hover .acts .ib, .row:focus-within .acts .ib, .row.selected .acts .ib, .row:hover .acts .more, .row:focus-within .acts .more { opacity: 1; }
  .ib { display: grid; place-items: center; width: 26px; height: 26px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
  .ib:hover:not(:disabled), .ib:focus-visible { background: var(--surface-active); color: var(--text-1); }
  .ib:disabled { cursor: default; }
  .row:hover .acts .ib:disabled, .row:focus-within .acts .ib:disabled { opacity: 0.35; }
  .ib dy-icon { --icon-size: 15px; }
  .h2 { margin: 0; font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-sans); }
  .lead { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
`;

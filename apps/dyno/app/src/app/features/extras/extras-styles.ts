/** The look the Extras tabs share: toolbar, table frame, designed empty and note states, row details. */
export const EXTRAS_STYLES = `
  :host { display: grid; gap: var(--sp-3); align-content: start; }
  dy-async { display: grid; gap: var(--sp-3); }
  .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
  .grow { flex: 1; }
  .faint { font-size: var(--fs-xs); }
  .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
  .note dy-icon { --icon-size: 15px; margin-top: 1px; color: var(--text-3); flex: none; }
  .note.bad, .note.bad dy-icon { color: var(--bad); }
  .sec { margin: var(--sp-2) 0 0; font-size: var(--fs-md); font-weight: var(--fw-semibold); display: flex; align-items: baseline; gap: var(--sp-2); }
  .sec .faint { font-weight: var(--fw-regular); }
  .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-8) var(--sp-5); text-align: center; color: var(--text-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
  .none dy-icon { --icon-size: 32px; }
  .none strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
  .none span { max-width: 60ch; font-size: var(--fs-sm); }
  .tile { display: inline-grid; place-items: center; width: 24px; height: 24px; margin-right: var(--sp-2); border-radius: var(--r-md); color: #fff; font-size: var(--fs-xs); font-weight: var(--fw-semibold); vertical-align: middle; }
  .dot { margin-right: var(--sp-2); vertical-align: 1px; }
  .path { font: var(--fw-regular) 12px / 1.4 var(--font-mono); user-select: text; overflow-wrap: anywhere; }
  .spin { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
`;

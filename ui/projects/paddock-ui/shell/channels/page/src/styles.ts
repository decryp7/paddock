/** The look of the Notification channels page (theme tokens only; no raw colours). Layout follows the container, not the window (rule 18). */
export const CHANNELS_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  /* The family's density and shape for the Material parts, so the page looks the same where an app's global theme is thin. */
  :host { --mat-button-filled-container-shape: var(--pd-radius-control); --mat-button-outlined-container-shape: var(--pd-radius-control); --mat-button-text-container-shape: var(--pd-radius-control);
    --mat-button-filled-container-height: var(--pd-control-height); --mat-button-outlined-container-height: var(--pd-control-height); --mat-button-text-container-height: var(--pd-control-height);
    --mat-form-field-container-height: var(--pd-control-height); --mat-form-field-container-vertical-padding: 0px; }
  .pd-ch__card { display: flex; flex-direction: column; gap: var(--pd-space-4); padding: var(--pd-space-4); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); min-width: 0; }
  .pd-ch__endpoint { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--pd-text-label); }
  h2, h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  .pd-ch__muted { color: var(--pd-text-muted); }
  .pd-ch__head { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: space-between; gap: var(--pd-space-2) var(--pd-space-4); }
  .pd-ch__head > div { min-width: 0; flex: 1 1 16rem; display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .pd-ch__table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  .pd-ch__table th { text-align: start; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
  .pd-ch__table td { padding: var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); vertical-align: top; min-width: 0; }
  .pd-ch__table tbody tr:hover, .pd-ch__table tbody tr:focus-within { background: var(--pd-hover); }
  .pd-ch__table tr.pd-ch__editing { background: var(--pd-surface-2); }
  .pd-ch__name { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .pd-ch__name strong { font-weight: var(--pd-weight-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pd-ch__col-type { width: 6rem; }
  .pd-ch__col-events { width: 36%; }
  .pd-ch__col-state { width: 8rem; }
  .pd-ch__col-actions { width: 7.5rem; }
  .pd-ch__states { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
  .pd-ch__actions { --mat-icon-button-state-layer-size: 32px; --mat-icon-button-icon-size: 16px; display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; gap: 2px; opacity: 0; }
  tr:hover .pd-ch__actions, tr:focus-within .pd-ch__actions, .pd-ch__actions--open { opacity: 1; }
  @media (hover: none) { .pd-ch__actions { opacity: 1; } }
  .pd-ch__test { display: inline-flex; align-items: center; gap: var(--pd-space-2); padding: 2px var(--pd-space-2); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); font-size: var(--pd-text-label); overflow-wrap: anywhere; min-width: 0; }
  .pd-ch__test--ok { background: var(--pd-success-tint); }
  .pd-ch__test--bad { background: var(--pd-danger-tint); }
  .pd-ch__spin { flex: none; width: 12px; height: 12px; border-radius: 50%; border: 2px solid var(--pd-border-control); border-top-color: var(--pd-accent); animation: pd-ch-spin 0.9s linear infinite; }
  @keyframes pd-ch-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .pd-ch__spin { animation: none; } }
  .pd-ch__progress { display: block; color: var(--pd-text-muted); }
  .pd-ch__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); min-width: 0; }
  .pd-ch__notice strong { font-weight: var(--pd-weight-medium); }
  .pd-ch__skeleton { display: flex; flex-direction: column; gap: var(--pd-space-2); }
  .pd-ch__editor { display: flex; flex-direction: column; gap: var(--pd-space-3); padding: var(--pd-space-4); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); min-width: 0; }
  .pd-ch__grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--pd-space-3); }
  .pd-ch__field { display: flex; flex-direction: column; gap: var(--pd-space-1); min-width: 0; font-weight: var(--pd-weight-medium); }
  .pd-ch__field--wide { grid-column: 1 / -1; }
  .pd-ch__field input[type='text'], .pd-ch__field input[type='number'], .pd-ch__field textarea { box-sizing: border-box; width: 100%; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: normal; min-width: 0; }
  .pd-ch__field textarea { padding: var(--pd-space-1) var(--pd-space-2); resize: vertical; }
  .pd-ch__field [aria-invalid='true'] { border-color: var(--pd-danger); }
  .pd-ch__hint { color: var(--pd-text-muted); font-weight: normal; font-size: var(--pd-text-caption); }
  .pd-ch__hint--bad { color: var(--pd-danger-text, var(--pd-text)); }
  .pd-ch__check { font-weight: var(--pd-weight-medium); }
  mat-checkbox.pd-ch__check { margin-inline-start: calc((var(--pd-control-height) - 18px) / -2); }
  .pd-ch__lan { display: flex; flex-direction: column; gap: var(--pd-space-1); }
  /* The help text starts where the switch's label does: the track and the gap before the label (Material's own tokens). */
  :host { --pd-ch-switch-indent: calc(var(--mat-slide-toggle-track-width, 52px) + var(--mat-slide-toggle-label-spacing, var(--pd-space-3))); }
  .pd-ch__hint--under-switch { margin-inline-start: var(--pd-ch-switch-indent); }
  /* On hover or keyboard focus within the row the host gives way to the whole address (the tooltip alone is not keyboard-reachable). */
  tr:hover .pd-ch__endpoint, tr:focus-within .pd-ch__endpoint { display: none; }
  tr:hover .pd-ch__full.pd-sr, tr:focus-within .pd-ch__full.pd-sr { position: static; width: auto; height: auto; overflow: visible; clip-path: none; white-space: normal; overflow-wrap: anywhere; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
  .pd-ch__grid > mat-slide-toggle { align-self: center; }
  .pd-ch__events-picker { margin: 0; padding: 0; border: 0; display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-ch__legend { font-weight: var(--pd-weight-medium); padding: 0; margin: 0 0 var(--pd-space-1); }
  .pd-ch__group { margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); display: flex; flex-direction: column; gap: var(--pd-space-1); min-width: 0; }
  .pd-ch__group legend { padding: 0 var(--pd-space-1); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
  .pd-ch__event { display: flex; flex-direction: column; }
  .pd-ch__event-help { margin-inline-start: calc(var(--pd-control-height) - 2px); margin-top: calc(var(--pd-space-1) * -1); }
  .pd-ch__chips { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
  .pd-ch__chip { display: inline-flex; align-items: center; gap: var(--pd-space-1); padding: 2px var(--pd-space-1) 2px var(--pd-space-2); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); font-size: var(--pd-text-label); }
  .pd-ch__chip-x { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: inherit; cursor: pointer; }
  .pd-ch__chip-x:hover { background: var(--pd-hover); }
  .pd-ch__other-add { display: flex; flex-wrap: wrap; align-items: flex-end; gap: var(--pd-space-2); }
  .pd-ch__other-add .pd-ch__field { flex: 1 1 12rem; }
  .pd-ch__secret { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2) var(--pd-space-3); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); grid-column: 1 / -1; }
  .pd-ch__foot { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
  .pd-ch__foot .pd-ch__spacer { flex: 1 1 0; }
  @container (max-width: 560px) {
    .pd-ch__grid { grid-template-columns: minmax(0, 1fr); }
    .pd-ch__col-type, .pd-ch__col-events, .pd-ch__type, .pd-ch__events { display: none; }
    .pd-ch__col-state { width: 7rem; }
    .pd-ch__col-actions { width: 5.5rem; }
  }
  @container (max-width: 420px) { .pd-ch__foot button { flex: 1 1 100%; } }
`;

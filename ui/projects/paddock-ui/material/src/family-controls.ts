/**
 * The family's Material density and shape (ER-21: one mechanism). `css/material.css` sets these on `:root`, but a host app's own
 * Material theme (`mat.theme` at another density, loaded after or before) can win there, and a Material field then draws 56px tall
 * with large type and a button as a pill. Custom properties set on a component's host beat any `:root` rule and reach everything
 * inside it, so every shared surface that draws Material controls (the Settings window and so all its pages, the notification
 * centre, `pd-select`, the segmented control) puts this block on its host: `styles: PD_FAMILY_CONTROLS + '...'`.
 * Only geometry and type are here, never colour. A test keeps it equal to the declarations in `scss/_material.scss`.
 */
export const PD_FAMILY_CONTROLS = `
  :host {
  --mat-button-filled-container-height: var(--pd-button-height);
  --mat-button-filled-container-shape: var(--pd-radius-control);
  --mat-button-filled-label-text-font: var(--pd-font-sans);
  --mat-button-filled-label-text-size: var(--pd-text-body);
  --mat-button-filled-label-text-weight: var(--pd-weight-medium);
  --mat-button-filled-label-text-transform: none;
  --mat-button-outlined-container-height: var(--pd-button-height);
  --mat-button-outlined-container-shape: var(--pd-radius-control);
  --mat-button-outlined-label-text-font: var(--pd-font-sans);
  --mat-button-outlined-label-text-size: var(--pd-text-body);
  --mat-button-outlined-label-text-weight: var(--pd-weight-medium);
  --mat-button-outlined-label-text-transform: none;
  --mat-button-text-container-height: var(--pd-button-height);
  --mat-button-text-container-shape: var(--pd-radius-control);
  --mat-button-text-label-text-font: var(--pd-font-sans);
  --mat-button-text-label-text-size: var(--pd-text-body);
  --mat-button-text-label-text-weight: var(--pd-weight-medium);
  --mat-button-text-label-text-transform: none;
  --mat-button-tonal-container-height: var(--pd-button-height);
  --mat-button-tonal-container-shape: var(--pd-radius-control);
  --mat-button-tonal-label-text-font: var(--pd-font-sans);
  --mat-button-tonal-label-text-size: var(--pd-text-body);
  --mat-button-tonal-label-text-weight: var(--pd-weight-medium);
  --mat-button-tonal-label-text-transform: none;
  --mat-button-protected-container-height: var(--pd-button-height);
  --mat-button-protected-container-shape: var(--pd-radius-control);
  --mat-button-protected-label-text-font: var(--pd-font-sans);
  --mat-button-protected-label-text-size: var(--pd-text-body);
  --mat-button-protected-label-text-weight: var(--pd-weight-medium);
  --mat-button-protected-label-text-transform: none;
  --mat-form-field-container-height: var(--pd-control-height);
  --mat-form-field-container-vertical-padding: 0px;
  --mat-form-field-container-text-font: var(--pd-font-sans);
  --mat-form-field-container-text-size: var(--pd-text-body);
  --mat-form-field-container-text-line-height: var(--pd-lh-body);
  --mat-form-field-outlined-container-shape: var(--pd-radius-input);
  --mat-select-trigger-text-font: var(--pd-font-sans);
  --mat-select-trigger-text-size: var(--pd-text-body);
  --mat-button-toggle-height: var(--pd-control-height);
  --mat-button-toggle-shape: var(--pd-radius-control);
  --mat-button-toggle-label-text-font: var(--pd-font-sans);
  --mat-button-toggle-label-text-size: var(--pd-text-body);
  --mat-button-toggle-label-text-line-height: var(--pd-lh-body);
  }
`;

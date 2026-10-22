/** Paddock-wide status roles. Status is never colour alone: every role has its own icon shape and the caller supplies a word. */
export type SuStatus = 'success' | 'warning' | 'danger' | 'info' | 'neutral';

export const SU_STATUSES: readonly SuStatus[] = ['success', 'warning', 'danger', 'info', 'neutral'];

/** 16 px icon paths (1.5 px stroke, round caps) per status: check circle, triangle, octagon-x, circle-i, dash circle. */
export const SU_STATUS_ICON: Record<SuStatus, string> = {
  success: 'M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 0 0 0-12.5ZM5.25 8.25l2 2 3.5-4',
  warning: 'M8 2.25 14 13H2L8 2.25ZM8 6.25v3M8 11.25v.01',
  danger: 'M5.2 1.75h5.6l3.45 3.45v5.6L10.8 14.25H5.2L1.75 10.8V5.2L5.2 1.75ZM6 6l4 4M10 6l-4 4',
  info: 'M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 0 0 0-12.5ZM8 7.25v3.5M8 5v.01',
  neutral: 'M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 0 0 0-12.5ZM5.5 8h5',
};

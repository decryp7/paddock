/** "now", "4 min ago", "3 h ago", "2 d ago" for a time `ms` since the epoch, given the clock `now` (passed in, never read here). */
export function relativeAgo(now: number, ms: number): string {
  const secs = Math.max(0, Math.round((now - ms) / 1000));
  if (secs < 60) return 'just now';
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

// A bundle or a file is storage: sizes come from the one formatter (decimal steps, like the operating systems show them).
export { formatStorage } from '@decryp7/paddock-ui/format';

/**
 * `text` cut in the middle to at most `max` characters (`/Users/you/…/report.zip`): the start and the end of a path or a
 * file name are what identify it (rule 18). The full value belongs in a tooltip.
 */
export function truncateMiddle(text: string, max: number): string {
  const chars = [...text];
  if (chars.length <= max || max < 3) return text;
  const keep = max - 1;
  const tail = Math.ceil(keep / 2);
  const head = keep - tail;
  return `${chars.slice(0, head).join('')}…${chars.slice(chars.length - tail).join('')}`;
}

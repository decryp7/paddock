/** Most ids the app puts in one bulk call. Far under the server's bound; the call's byte limit is what decides for long ids. */
export const MAX_IDS_PER_CALL = 512;

/**
 * Splits `ids` into calls that each stay under `maxBytes` of JSON and `maxIds` ids, in order, so a bulk call never meets the
 * command's request limit (`APP_COMMANDS.<command>.limit`) or the server's bound (ADR 0108: bulk calls stay bounded). One id
 * larger than the limit goes alone: the server answers it as that id's own `invalid` result.
 */
export function chunkIds(ids: readonly string[], maxBytes: number, maxIds: number = MAX_IDS_PER_CALL): string[][] {
  const encoder = new TextEncoder();
  const chunks: string[][] = [];
  let current: string[] = [];
  let bytes = 2; // the brackets of the array
  for (const id of ids) {
    const size = encoder.encode(JSON.stringify(id)).length + 1; // the comma
    if (current.length > 0 && (current.length >= maxIds || bytes + size > maxBytes)) {
      chunks.push(current);
      current = [];
      bytes = 2;
    }
    current.push(id);
    bytes += size;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

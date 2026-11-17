let last = 0;

/**
 * One monotonic sequence for the live-tail grid's inputs (ADR 0072, decision 3, G10). A request that moves the view (a reveal, Follow on,
 * a resize re-pin, a new query) is stamped when it is made; a user take-over is stamped when the input arrives. The follow reducer drops a
 * request stamped before the last take-over, so an effect that applies a request late can never undo what the user did after it. Stamps start at 1.
 */
export function nextRequestSeq(): number {
  return ++last;
}

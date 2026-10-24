/**
 * A category label inside a sentence: "Logs" becomes "logs" ("Clear logs?"), but a label that starts with an acronym or a name
 * written in capitals ("SQL stores", "AI models") keeps its capitals. Pure.
 */
export function inSentence(label: string): string {
  const text = label.trim();
  if (text.length < 2) return text.toLowerCase();
  const second = text[1] ?? '';
  const upperSecond = second !== second.toLowerCase();
  return upperSecond ? text : (text[0] ?? '').toLowerCase() + text.slice(1);
}

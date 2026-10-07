/**
 * The median the run page's tables and charts show. In: any list of numbers (TTKs, flick times,
 * distances). Out: the kills table, the click and tracking reports, and the run charts.
 */

/** The middle value, or the mean of the two middle values; null for no values. */
export function median(values: number[]): number | null {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const half = sorted.length >> 1;
  return sorted.length % 2 ? sorted[half] : (sorted[half - 1] + sorted[half]) / 2;
}

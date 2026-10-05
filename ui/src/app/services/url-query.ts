/** A value of the page's URL query; null when it has none. */
export function queryValue(key: string): string | null {
  return new URLSearchParams(location.search).get(key);
}

/**
 * Changes some values of the page's URL query and keeps the rest (the open recording's ?id=, the Crops page's ?page=):
 * a key set to null is taken out. The page is not reloaded and no history entry is added.
 */
export function setQuery(changes: Record<string, string | null>): void {
  const params = new URLSearchParams(location.search);
  for (const [key, value] of Object.entries(changes)) {
    if (value === null) params.delete(key);
    else params.set(key, value);
  }
  const query = params.toString();
  history.replaceState(null, '', query ? `?${query}` : location.pathname);
}

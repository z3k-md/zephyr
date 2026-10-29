/** A Google search for `text`. */
export function googleUrl(text: string): string {
  return `https://www.google.com/search?q=${encodeURIComponent(text)}`;
}

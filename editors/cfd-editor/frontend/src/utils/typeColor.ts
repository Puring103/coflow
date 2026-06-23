/**
 * Stable per-type color derived from a hash of the type name.
 *
 * The lightness is theme-aware: dark theme uses a higher lightness so colors
 * read against dark surfaces, light theme uses a lower lightness so they
 * stay readable against white. We read `data-theme` off <html> at call time
 * so a theme toggle immediately recolors without a re-render — callers that
 * bake the result into inline styles will recompute on their next render
 * anyway, and CSS custom-property consumers (var(--node-color)) get the new
 * value the next time the style is applied.
 */
export function typeColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  const hue = hash % 360
  const dark = typeof document === 'undefined'
    ? true
    : document.documentElement.getAttribute('data-theme') !== 'light'
  return `hsl(${hue}, 55%, ${dark ? 62 : 42}%)`
}

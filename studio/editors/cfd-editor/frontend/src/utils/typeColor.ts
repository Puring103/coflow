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

/** Stable per-enum-name color. Uses the same hash algorithm as typeColor but
 *  with slightly lower saturation so enum chips are visually distinct from
 *  type badges while still having strong per-name identity. */
export function enumColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  const hue = hash % 360
  const dark = typeof document === 'undefined'
    ? true
    : document.documentElement.getAttribute('data-theme') !== 'light'
  return `hsl(${hue}, 45%, ${dark ? 58 : 38}%)`
}

/** Color used by field type hints and editors. Primitive types keep a
 * familiar semantic color while named schema types remain stable per name. */
export function fieldTypeColor(declaredType: string): string {
  const normalized = declaredType.trim().replace(/\?$/, '').replace(/^&/, '')
  const primitive = normalized.toLowerCase()
  const dark = typeof document === 'undefined'
    ? true
    : document.documentElement.getAttribute('data-theme') !== 'light'
  const lightness = dark ? 62 : 40
  if (primitive === 'bool') return `hsl(145, 48%, ${lightness}%)`
  if (primitive === 'int' || primitive === 'float') return `hsl(35, 72%, ${lightness}%)`
  if (primitive === 'string') return `hsl(190, 52%, ${lightness}%)`
  return typeColor(normalized || declaredType)
}

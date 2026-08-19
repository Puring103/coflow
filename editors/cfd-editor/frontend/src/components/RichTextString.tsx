import type { CSSProperties, ReactNode } from 'react'

type RichNode =
  | { kind: 'text', value: string }
  | { kind: 'tag', name: string, attributes: string, children: RichNode[] }

const CONTAINER_TAGS = new Set([
  'a', 'alpha', 'b', 'big', 'blockquote', 'center', 'code', 'color', 'del', 'div', 'em',
  'font', 'font-weight', 'gradient', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'i', 'indent',
  'li', 'link', 'lowercase', 'mark', 'nobr', 'ol', 'p', 'pre', 'rotate', 's', 'size',
  'small', 'smallcaps', 'span', 'strike', 'strong', 'style', 'sub', 'sup', 'u', 'ul',
  'uppercase', 'voffset',
])
const VOID_TAGS = new Set(['br', 'hr', 'img', 'space', 'sprite'])

export function RichTextString({ text, renderText }: {
  text: string
  renderText?: (text: string) => ReactNode
}) {
  const nodes = parseRichText(text)
  if (!nodes) return <>{renderText ? renderText(text) : text}</>
  return <span className="rich-text-string">{renderNodes(nodes, renderText)}</span>
}

export function parseRichText(text: string): RichNode[] | null {
  const root: RichNode[] = []
  const stack: Array<{ name: string, children: RichNode[] }> = [{ name: '', children: root }]
  const pattern = /<\/?([A-Za-z][\w-]*)([^<>]*)>/g
  let recognized = false
  let offset = 0
  let match: RegExpExecArray | null
  while ((match = pattern.exec(text)) !== null) {
    const raw = match[0]
    const name = match[1].toLowerCase()
    const closing = raw.startsWith('</')
    const supported = CONTAINER_TAGS.has(name) || VOID_TAGS.has(name)
    if (!supported) continue
    if (match.index > offset) pushText(stack[stack.length - 1].children, text.slice(offset, match.index))
    recognized = true
    offset = pattern.lastIndex
    if (closing) {
      const openIndex = stack.map(item => item.name).lastIndexOf(name)
      if (openIndex > 0) stack.length = openIndex
      continue
    }
    const node: RichNode = { kind: 'tag', name, attributes: match[2].trim(), children: [] }
    stack[stack.length - 1].children.push(node)
    if (!VOID_TAGS.has(name) && !raw.endsWith('/>')) stack.push({ name, children: node.children })
  }
  if (!recognized) return null
  if (offset < text.length) pushText(stack[stack.length - 1].children, text.slice(offset))
  return root
}

function pushText(nodes: RichNode[], value: string) {
  if (value) nodes.push({ kind: 'text', value: decodeEntities(value) })
}

function renderNodes(nodes: RichNode[], renderText?: (text: string) => ReactNode): ReactNode[] {
  return nodes.map((node, index) => {
    if (node.kind === 'text') return <span key={index}>{renderText ? renderText(node.value) : node.value}</span>
    const children = renderNodes(node.children, renderText)
    const key = `${node.name}:${index}`
    switch (node.name) {
      case 'b': case 'strong': return <strong key={key}>{children}</strong>
      case 'i': case 'em': return <em key={key}>{children}</em>
      case 'u': return <span key={key} style={{ textDecoration: 'underline' }}>{children}</span>
      case 's': case 'strike': case 'del': return <s key={key}>{children}</s>
      case 'sub': return <sub key={key}>{children}</sub>
      case 'sup': return <sup key={key}>{children}</sup>
      case 'small': return <small key={key}>{children}</small>
      case 'big': return <span key={key} style={{ fontSize: 'larger' }}>{children}</span>
      case 'code': return <code key={key}>{children}</code>
      case 'pre': return <span className="rich-pre" key={key}>{children}</span>
      case 'mark': return <mark key={key} style={markStyle(node.attributes)}>{children}</mark>
      case 'color': case 'font': return <span key={key} style={colorStyle(node.attributes)}>{children}</span>
      case 'size': return <span key={key} style={sizeStyle(node.attributes)}>{children}</span>
      case 'font-weight': return <span key={key} style={weightStyle(node.attributes)}>{children}</span>
      case 'uppercase': return <span key={key} style={{ textTransform: 'uppercase' }}>{children}</span>
      case 'lowercase': return <span key={key} style={{ textTransform: 'lowercase' }}>{children}</span>
      case 'smallcaps': return <span key={key} style={{ fontVariant: 'small-caps' }}>{children}</span>
      case 'nobr': return <span key={key} style={{ whiteSpace: 'nowrap' }}>{children}</span>
      case 'a': case 'link': return <span className="rich-link" key={key}>{children}</span>
      case 'br': return <br key={key} />
      case 'hr': return <span className="rich-rule" key={key} />
      case 'sprite': return <span className="rich-asset" key={key}>□</span>
      case 'img': return <span className="rich-asset" key={key}>▧</span>
      case 'space': return <span key={key} style={{ display: 'inline-block', width: scalarAttribute(node.attributes, '0.5em') }} />
      case 'p': case 'div': case 'blockquote': case 'li':
      case 'h1': case 'h2': case 'h3': case 'h4': case 'h5': case 'h6':
        return <span className={`rich-block rich-${node.name}`} key={key}>{children}</span>
      case 'span': return <span key={key} style={htmlSpanStyle(node.attributes)}>{children}</span>
      default: return <span key={key}>{children}</span>
    }
  })
}

function scalarAttribute(attributes: string, fallback: string): string {
  const raw = attributes.match(/^=\s*["']?([^"'\s]+)|\b(?:size|value)\s*=\s*["']?([^"'\s]+)/i)
  return raw?.[1] ?? raw?.[2] ?? fallback
}

function safeColor(value: string | undefined): string | undefined {
  if (!value) return undefined
  return /^(?:#[0-9a-f]{3,8}|[a-z]{3,20}|rgba?\([\d\s.,%]+\)|hsla?\([\d\s.,%]+\))$/i.test(value)
    ? value
    : undefined
}

function colorStyle(attributes: string): CSSProperties {
  const unity = scalarAttribute(attributes, '')
  const html = attributes.match(/(?:color|style\s*=\s*["'][^"']*color\s*:)\s*["']?([^;"'\s]+)/i)?.[1]
  return { color: safeColor(html || unity) }
}

function sizeStyle(attributes: string): CSSProperties {
  const value = scalarAttribute(attributes, '')
  if (!/^[+-]?(?:\d+(?:\.\d+)?)(?:px|em|rem|%)?$/.test(value)) return {}
  return { fontSize: /^\d+(?:\.\d+)?$/.test(value) ? `${value}px` : value }
}

function weightStyle(attributes: string): CSSProperties {
  const value = scalarAttribute(attributes, '')
  return /^(?:normal|bold|[1-9]00)$/.test(value) ? { fontWeight: value } : {}
}

function markStyle(attributes: string): CSSProperties {
  const color = safeColor(scalarAttribute(attributes, ''))
  return color ? { backgroundColor: color } : {}
}

function htmlSpanStyle(attributes: string): CSSProperties {
  const style = attributes.match(/\bstyle\s*=\s*["']([^"']*)["']/i)?.[1] ?? ''
  const color = safeColor(style.match(/(?:^|;)\s*color\s*:\s*([^;]+)/i)?.[1]?.trim())
  const fontWeight = style.match(/(?:^|;)\s*font-weight\s*:\s*(normal|bold|[1-9]00)/i)?.[1]
  const fontStyle = style.match(/(?:^|;)\s*font-style\s*:\s*(normal|italic)/i)?.[1]
  const textDecoration = style.match(/(?:^|;)\s*text-decoration\s*:\s*(none|underline|line-through)/i)?.[1]
  return { color, fontWeight, fontStyle, textDecoration }
}

function decodeEntities(value: string): string {
  return value.replace(/&(?:amp|lt|gt|quot|apos|#\d+|#x[0-9a-f]+);/gi, entity => {
    const named: Record<string, string> = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&apos;': "'" }
    const known = named[entity.toLowerCase()]
    if (known) return known
    const hex = entity.match(/^&#x([0-9a-f]+);$/i)
    const decimal = entity.match(/^&#(\d+);$/)
    const code = hex ? Number.parseInt(hex[1], 16) : decimal ? Number.parseInt(decimal[1], 10) : NaN
    return Number.isFinite(code) ? String.fromCodePoint(code) : entity
  })
}

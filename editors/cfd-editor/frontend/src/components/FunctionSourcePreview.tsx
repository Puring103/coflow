interface PreviewToken {
  text: string
  type?: string
}

const keywords = new Set([
  'break', 'continue', 'else', 'false', 'fn', 'for', 'if', 'in', 'let', 'match',
  'None', 'Ok', 'Err', 'return', 'Some', 'true', 'var', 'while',
])

export function functionBodyBounds(source: string): { from: number; to: number } | null {
  let bodyStart = -1
  let depth = 0
  let quote = ''
  let escaped = false
  let lineComment = false

  for (let index = 0; index < source.length; index += 1) {
    const character = source[index]
    const next = source[index + 1]
    if (lineComment) {
      if (character === '\n') lineComment = false
      continue
    }
    if (quote) {
      if (escaped) escaped = false
      else if (character === '\\') escaped = true
      else if (character === quote) quote = ''
      continue
    }
    if (character === '#' || (character === '/' && next === '/')) {
      lineComment = true
      if (character === '/') index += 1
      continue
    }
    if (character === '"' || character === "'") {
      quote = character
      continue
    }
    if (character === '{') {
      if (bodyStart < 0) bodyStart = index + 1
      depth += 1
    } else if (character === '}' && bodyStart >= 0) {
      depth -= 1
      if (depth === 0) return { from: bodyStart, to: index }
    }
  }
  return null
}

export function functionBody(source: string): string {
  const bounds = functionBodyBounds(source)
  return bounds ? source.slice(bounds.from, bounds.to).trim() : source.trim()
}

export function highlightFunctionBody(source: string): PreviewToken[] {
  const body = functionBody(source)
  const tokens: PreviewToken[] = []
  const pattern = /\s+|\/\/[^\n]*|#[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b\d+(?:\.\d+)?\b|[\p{L}_][\p{L}\p{N}_]*|[^\s]/gu
  for (const match of body.matchAll(pattern)) {
    const text = match[0]
    let type: string | undefined
    if (/^\s+$/.test(text)) {
      if (tokens.length && tokens[tokens.length - 1]?.text !== ' ') tokens.push({ text: ' ' })
      continue
    }
    if (text.startsWith('//') || text.startsWith('#')) type = 'comment'
    else if (text.startsWith('"') || text.startsWith("'")) type = 'string'
    else if (/^\d/.test(text)) type = 'number'
    else if (keywords.has(text)) type = 'keyword'
    else if (/^[+*/%=&|!<>?:.,;()[\]{}-]$/.test(text)) type = 'operator'
    else if (/^[\p{L}_]/u.test(text)) type = 'parameter'
    tokens.push({ text, type })
  }
  while (tokens[tokens.length - 1]?.text === ' ') tokens.pop()
  return tokens
}

export function FunctionSourcePreview({ source }: { source: string }) {
  return (
    <code className="function-source-preview">
      {highlightFunctionBody(source).map((token, index) => token.type
        ? <span className={`cm-lsp-token-${token.type}`} key={index}>{token.text}</span>
        : token.text)}
    </code>
  )
}

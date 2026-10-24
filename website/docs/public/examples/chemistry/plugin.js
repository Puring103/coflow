// 仅排版普通化学式与反应箭头，不做配平或化学有效性校验。
export function renderFormula(expression) {
  const formula = document.createElement('span')
  formula.setAttribute('role', 'img')
  formula.setAttribute('aria-label', expression)
  formula.style.cssText = 'font-family:Georgia,"Times New Roman",serif;white-space:pre-wrap;overflow-wrap:anywhere;line-height:1.8'
  const text = expression.replaceAll('<->', '⇌').replaceAll('->', '→')
  // 元素或括号后的数字表示下标；独立的反应系数保持基线显示。
  for (const match of text.matchAll(/[A-Za-z)\]]\d+|[^]/g)) {
    const token = match[0]
    if (/^[A-Za-z)\]]\d+$/.test(token)) {
      formula.append(document.createTextNode(token[0]))
      const sub = document.createElement('sub')
      sub.style.cssText = 'font-size:.65em;line-height:0'
      sub.textContent = token.slice(1)
      formula.append(sub)
    } else {
      formula.append(document.createTextNode(token))
    }
  }
  return formula
}

export function mountFormula(context, outlet) {
  const update = (next) => {
    const expression = next.value.value.fields.expression.value
    outlet.replace(renderFormula(expression))
  }
  update(context)
  return { update }
}

export function activate(host) {
  const unregister = ['cell', 'inspector'].map(slot => host.register.presentation({
    id: `chemical-formula-${slot}`, types: ['Formula'], slot, default: true,
    mount: mountFormula,
  }))
  return { dispose() { unregister.forEach(dispose => dispose()) } }
}

export default activate

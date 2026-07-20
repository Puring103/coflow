function field(value, name) {
  return value && value.kind === 'object' ? value.value.fields[name] : undefined
}

function terms(value) {
    if (!value || value.kind !== 'array') return []
    return value.value.flatMap(item => {
      const material = field(item, 'material')
      const parameters = field(item, 'parameters')
      const coefficient = field(parameters, 'coefficient')
      if (!material || material.kind !== 'enum') return []
      return [{
        material: material.value.variant || String(material.value.value),
        coefficient: coefficient && (coefficient.kind === 'int' || coefficient.kind === 'float')
          ? String(coefficient.value)
          : '1',
      }]
    })
  }

function formula(text) {
  const fragment = document.createDocumentFragment()
  for (const part of text.split(/(\d+)/)) {
    const node = /^\d+$/.test(part) ? document.createElement('sub') : document.createTextNode(part)
    node.textContent = part
    fragment.append(node)
  }
  return fragment
}

function side(items) {
  const element = document.createElement('span')
  element.className = 'chemical-equation-side'
  items.forEach((item, index) => {
    if (index) {
      const plus = document.createElement('span')
      plus.className = 'chemical-equation-plus'
      plus.textContent = '+'
      element.append(plus)
    }
    if (item.coefficient !== '1') element.append(document.createTextNode(item.coefficient))
    element.append(formula(item.material))
  })
  return element
}

export default function activate(host) {
  host.renderers.register({
    id: 'chemical-expression',
    target: {
      kind: 'field-value',
      type: 'ChemicalExpression',
      surfaces: ['table-cell', 'record-foldout-header'],
    },
    mount(context, outlet) {
      const inputs = terms(field(context.value, 'inputs'))
      const outputs = terms(field(context.value, 'outputs'))
      if (!inputs.length || !outputs.length) {
        outlet.replace('化学方程式')
        return
      }
      const element = document.createElement('span')
      element.className = 'chemical-equation chemical-equation-compact'
      element.append(side(inputs))
      const arrow = document.createElement('span')
      arrow.className = 'chemical-equation-arrow'
      arrow.textContent = '→'
      element.append(arrow, side(outputs))
      outlet.replace(element)
    },
  })
}

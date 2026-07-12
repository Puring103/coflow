import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import ELK from 'elkjs/lib/elk.bundled.js'
import { test } from 'vitest'
import {
  defaultEnabledFields,
  estimateNodeHeight,
  isCompactGraphZoom,
  layoutGraph,
} from './GraphView.layout'

const styles = readFileSync(new URL('../style.css', import.meta.url), 'utf8')
const elk = new ELK()

async function runElkLayout(graph) {
  const laidOut = await elk.layout(graph)
  const children = laidOut.children ?? []
  const minX = children.length > 0 ? Math.min(...children.map(node => node.x ?? 0)) : 0
  return new Map(children.map(node => [
    node.id,
    { x: (node.x ?? 0) - minX, y: node.y ?? 0 },
  ]))
}

function layoutAll(graph, enabledFields, activeType, nodeExpanded, rowExpanded) {
  return layoutGraph(
    graph,
    enabledFields,
    activeType,
    nodeExpanded,
    rowExpanded,
    runElkLayout,
  )
}

function graphNode(key, fields = [], actualType = 'Item') {
  return {
    id: `${actualType}::${key}`,
    key,
    actual_type: actualType,
    coordinate: { actual_type: actualType, key },
    file_path: 'data/items.cfd',
    in_focus_file: true,
    is_collapsed: false,
    fields,
    field_diagnostics: [],
    diagnostic_severity: null,
  }
}

function readCssRule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const match = styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))
  assert.ok(match, `missing CSS rule ${selector}`)
  return match[1]
}

function readCssDeclaration(selector, property) {
  const rule = readCssRule(selector)
  const escaped = property.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const match = rule.match(new RegExp(`${escaped}\\s*:\\s*([^;]+);?`))
  return match?.[1]?.trim()
}

function crossingCount(edges, positions) {
  let count = 0
  const visibleEdges = edges.filter(e => positions.has(e.source) && positions.has(e.target))
  for (let i = 0; i < visibleEdges.length; i++) {
    for (let j = i + 1; j < visibleEdges.length; j++) {
      const a = visibleEdges[i]
      const b = visibleEdges[j]
      const sourceA = positions.get(a.source)
      const sourceB = positions.get(b.source)
      const targetA = positions.get(a.target)
      const targetB = positions.get(b.target)
      if (sourceA.x !== sourceB.x || targetA.x !== targetB.x) continue
      const sourceDelta = sourceA.y - sourceB.y
      const targetDelta = targetA.y - targetB.y
      if (sourceDelta * targetDelta < 0) count++
    }
  }
  return count
}

test('uses compact graph nodes when zoomed far out', () => {
  assert.equal(isCompactGraphZoom(0.64), true)
  assert.equal(isCompactGraphZoom(0.65), false)
  assert.equal(isCompactGraphZoom(0.9), false)
})

test('compact mode keeps the same estimated card height', () => {
  const node = graphNode('A', [
    { name: 'a', value: { kind: 'scalar', value: 1 }, annotation: null },
    { name: 'b', value: { kind: 'scalar', value: 2 }, annotation: null },
    { name: 'c', value: { kind: 'scalar', value: 3 }, annotation: null },
    { name: 'd', value: { kind: 'scalar', value: 4 }, annotation: null },
    { name: 'e', value: { kind: 'scalar', value: 5 }, annotation: null },
  ])

  assert.equal(estimateNodeHeight(node, false, new Set()), 164)
})

test('compact graph nodes keep the normal card width', () => {
  assert.equal(readCssDeclaration('.graph-node', 'width'), '280px')
  assert.notEqual(readCssDeclaration('.graph-node.compact', 'width'), '180px')
})

test('compact graph IDs are large and can wrap for readability', () => {
  const fontSize = Number.parseFloat(readCssDeclaration('.gn-compact-key', 'font-size'))

  assert.ok(fontSize >= 30)
  assert.equal(readCssDeclaration('.gn-compact-key', 'white-space'), 'normal')
  assert.equal(readCssDeclaration('.gn-compact-key', 'text-overflow'), undefined)
})

test('lays out same-table references across successive columns', async () => {
  const graph = {
    nodes: [
      graphNode('A', [{ name: 'next', value: { kind: 'ref', value: '' }, annotation: null }]),
      graphNode('B', [{ name: 'next', value: { kind: 'ref', value: '' }, annotation: null }]),
      graphNode('C'),
    ],
    edges: [
      { source: 'Item::A', target: 'Item::B', field_path: 'next' },
      { source: 'Item::B', target: 'Item::C', field_path: 'next' },
    ],
  }

  const layoutPromise = layoutAll(graph, new Set(['next']), 'Item', new Map(), new Map())
  assert.equal(typeof layoutPromise.then, 'function')
  const result = await layoutPromise

  assert.equal(result.positions.get('Item::A').x, 0)
  assert.ok(result.positions.get('Item::B').x > result.positions.get('Item::A').x)
  assert.ok(result.positions.get('Item::C').x > result.positions.get('Item::B').x)
})

test('shows cross-type relationships when their field is enabled', async () => {
  const graph = {
    nodes: [
      graphNode('A', [
        { name: 'next', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'quest', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('B'),
      graphNode('Q', [], 'Quest'),
    ],
    edges: [
      { source: 'Item::A', target: 'Item::B', field_path: 'next' },
      { source: 'Item::A', target: 'Quest::Q', field_path: 'quest' },
    ],
  }

  const result = await layoutAll(graph, new Set(['next', 'quest']), 'Item', new Map(), new Map())

  assert.deepEqual(
    result.visibleNodes.map(n => n.id).sort(),
    ['Item::A', 'Item::B', 'Quest::Q'],
  )
  assert.deepEqual(
    result.forwardEdges.map(e => `${e.source}->${e.target}`).sort(),
    ['Item::A->Item::B', 'Item::A->Quest::Q'],
  )
})

test('defaults enabled fields to active-type relationships only', () => {
  const graph = {
    nodes: [
      graphNode('A'),
      graphNode('B'),
      graphNode('Q', [], 'Quest'),
    ],
    edges: [
      { source: 'Item::A', target: 'Item::B', field_path: 'next' },
      { source: 'Item::A', target: 'Quest::Q', field_path: 'quest' },
    ],
  }

  assert.deepEqual(
    defaultEnabledFields(graph, ['next', 'quest'], 'Item'),
    ['next'],
  )
})

test('keeps records without same-type incoming edges in the leftmost column', async () => {
  const graph = {
    nodes: [
      graphNode('Child'),
      graphNode('Root', [
        { name: 'child', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
    ],
    edges: [
      { source: 'Item::Root', target: 'Item::Child', field_path: 'child' },
    ],
  }

  const result = await layoutAll(graph, new Set(['child']), 'Item', new Map(), new Map())

  assert.equal(result.positions.get('Item::Root').x, 0)
  assert.ok(result.positions.get('Item::Child').x > result.positions.get('Item::Root').x)
})

test('reduces crossings in dense adjacent layers', async () => {
  const graph = {
    nodes: [
      graphNode('A', [
        { name: 'w', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'y', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'z', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('B', [
        { name: 'w', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'x', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('C', [
        { name: 'w', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('D'),
      graphNode('W'),
      graphNode('X'),
      graphNode('Y'),
      graphNode('Z'),
    ],
    edges: [
      { source: 'Item::A', target: 'Item::W', field_path: 'w' },
      { source: 'Item::A', target: 'Item::Y', field_path: 'y' },
      { source: 'Item::A', target: 'Item::Z', field_path: 'z' },
      { source: 'Item::B', target: 'Item::W', field_path: 'w' },
      { source: 'Item::B', target: 'Item::X', field_path: 'x' },
      { source: 'Item::C', target: 'Item::W', field_path: 'w' },
    ],
  }

  const result = await layoutAll(graph, new Set(['w', 'x', 'y', 'z']), 'Item', new Map(), new Map())

  assert.ok(crossingCount(result.forwardEdges, result.positions) <= 1)
})

test('uses downstream relationships to reduce crossings across three layers', async () => {
  const graph = {
    nodes: [
      graphNode('C', [
        { name: 'm', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'n', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'o', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('M', [
        { name: 'z', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('N', [
        { name: 'y', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('O', [
        { name: 'x', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'z', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('X'),
      graphNode('Y'),
      graphNode('Z'),
    ],
    edges: [
      { source: 'Item::C', target: 'Item::M', field_path: 'm' },
      { source: 'Item::C', target: 'Item::N', field_path: 'n' },
      { source: 'Item::C', target: 'Item::O', field_path: 'o' },
      { source: 'Item::M', target: 'Item::Z', field_path: 'z' },
      { source: 'Item::N', target: 'Item::Y', field_path: 'y' },
      { source: 'Item::O', target: 'Item::X', field_path: 'x' },
      { source: 'Item::O', target: 'Item::Z', field_path: 'z' },
    ],
  }

  const result = await layoutAll(graph, new Set(['m', 'n', 'o', 'x', 'y', 'z']), 'Item', new Map(), new Map())

  assert.equal(crossingCount(result.forwardEdges, result.positions), 0)
})

test('spreads middle-layer records by the occupied height of their downstream nodes', async () => {
  const graph = {
    nodes: [
      graphNode('Root', [
        { name: 'a', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'b', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('A', [
        { name: 'a1', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'a2', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'a3', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'a4', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('B', [
        { name: 'b1', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'b2', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'b3', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'b4', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('A1'),
      graphNode('A2'),
      graphNode('A3'),
      graphNode('A4'),
      graphNode('B1'),
      graphNode('B2'),
      graphNode('B3'),
      graphNode('B4'),
    ],
    edges: [
      { source: 'Item::Root', target: 'Item::A', field_path: 'a' },
      { source: 'Item::Root', target: 'Item::B', field_path: 'b' },
      { source: 'Item::A', target: 'Item::A1', field_path: 'a1' },
      { source: 'Item::A', target: 'Item::A2', field_path: 'a2' },
      { source: 'Item::A', target: 'Item::A3', field_path: 'a3' },
      { source: 'Item::A', target: 'Item::A4', field_path: 'a4' },
      { source: 'Item::B', target: 'Item::B1', field_path: 'b1' },
      { source: 'Item::B', target: 'Item::B2', field_path: 'b2' },
      { source: 'Item::B', target: 'Item::B3', field_path: 'b3' },
      { source: 'Item::B', target: 'Item::B4', field_path: 'b4' },
    ],
  }

  const result = await layoutAll(graph, new Set(['a', 'b', 'a1', 'a2', 'a3', 'a4', 'b1', 'b2', 'b3', 'b4']), 'Item', new Map(), new Map())
  const middleGap = Math.abs(result.positions.get('Item::B').y - result.positions.get('Item::A').y)

  assert.ok(middleGap >= 340)
})

test('spreads roots by the occupied height of their downstream nodes', async () => {
  const graph = {
    nodes: [
      graphNode('A', [
        { name: 'm', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('B', [
        { name: 'm', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('M', [
        { name: 'l1', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'l2', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'l3', value: { kind: 'ref', value: '' }, annotation: null },
        { name: 'l4', value: { kind: 'ref', value: '' }, annotation: null },
      ]),
      graphNode('L1'),
      graphNode('L2'),
      graphNode('L3'),
      graphNode('L4'),
    ],
    edges: [
      { source: 'Item::A', target: 'Item::M', field_path: 'm' },
      { source: 'Item::B', target: 'Item::M', field_path: 'm' },
      { source: 'Item::M', target: 'Item::L1', field_path: 'l1' },
      { source: 'Item::M', target: 'Item::L2', field_path: 'l2' },
      { source: 'Item::M', target: 'Item::L3', field_path: 'l3' },
      { source: 'Item::M', target: 'Item::L4', field_path: 'l4' },
    ],
  }

  const result = await layoutAll(graph, new Set(['m', 'l1', 'l2', 'l3', 'l4']), 'Item', new Map(), new Map())
  const a = result.positions.get('Item::A')
  const b = result.positions.get('Item::B')
  const aHeight = estimateNodeHeight(graph.nodes.find(n => n.id === 'Item::A'), false, new Set())
  const bHeight = estimateNodeHeight(graph.nodes.find(n => n.id === 'Item::B'), false, new Set())
  const [top, bottom, topHeight] = a.y <= b.y ? [a, b, aHeight] : [b, a, bHeight]

  assert.ok(bottom.y >= top.y + topHeight)
})

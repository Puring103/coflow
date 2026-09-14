import assert from 'node:assert/strict'
import { test } from 'vitest'
import { roundedPolyline, shaderEdgePath } from './GraphView.edges'

test('straight connections stay three collinear segments with no rounding', () => {
  const path = shaderEdgePath({ x: 0, y: 0 }, { x: 200, y: 0 })

  assert.equal(path, 'M 0 0 L 28 0 L 172 0 L 200 0')
  assert.equal(path.includes(' Q '), false)
})

test('sharp corners are rounded into quadratic curves', () => {
  const path = shaderEdgePath({ x: 0, y: 0 }, { x: 120, y: 160 })

  assert.ok(path.includes(' Q '))
  assert.ok(path.endsWith('L 120 160'))
})

test('sharper corners round more than shallower corners', () => {
  const firstCornerEntryX = path => Number(path.match(/M [\d.-]+ [\d.-]+ L ([\d.-]+)/)[1])
  const shallow = shaderEdgePath({ x: 0, y: 0 }, { x: 200, y: 60 })
  const steep = shaderEdgePath({ x: 0, y: 0 }, { x: 200, y: 200 })

  // 圆角越大，进入折角的 x 越靠近源端，即越小。
  assert.ok(firstCornerEntryX(steep) < firstCornerEntryX(shallow))
})

test('stubs always point outwards from their node, even for backward targets', () => {
  // 源端口向右伸出，目标端口向左伸出，回边也不会伸进节点内部。
  const path = shaderEdgePath({ x: 300, y: 0 }, { x: 0, y: 50 })
  const numbers = (path.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number)

  assert.ok(path.startsWith('M 300 0'))
  assert.ok(Math.max(...numbers) > 300, 'source stub should extend right of the source port')
  assert.ok(Math.min(...numbers) < 0, 'target stub should extend left of the target port')
})

test('rounding never exceeds half of the shortest adjacent segment', () => {
  // 折点两端各长 10，圆角半径上限应为 5，折线不会越过相邻点。
  const path = roundedPolyline([
    { x: 0, y: 0 },
    { x: 10, y: 0 },
    { x: 10, y: 10 },
    { x: 0, y: 10 },
  ])

  assert.ok(path.includes(' Q '))
  const numbers = path.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? []
  for (const value of numbers) {
    assert.ok(value >= 0 && value <= 10, `coordinate ${value} escaped the segment bounds`)
  }
})

import { createElement } from 'react'
import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { DataCardCompact } from './DataCard'
import type { FieldValue } from '../wire'

describe('DataCardCompact complex previews', () => {
  it('renders a bounded markdown tree while preserving scalar value styles', () => {
    const value: FieldValue = {
      kind: 'object',
      value: {
        actual_type: 'Item',
        fields: {
          hiddenLeafLabel: { kind: 'ref', value: '&ItemConfig.sword' },
          rarity: {
            kind: 'enum',
            value: { enum_name: 'Rarity', variant: 'Epic', value: 2n },
          },
          rewards: {
            kind: 'array',
            value: [
              { kind: 'string', value: 'gold' },
              { kind: 'string', value: 'silver' },
              { kind: 'string', value: 'wood' },
              { kind: 'string', value: 'stone' },
              { kind: 'string', value: 'iron' },
            ],
          },
          rates: {
            kind: 'dict',
            value: [[{ kind: 'string', value: 'mobile' }, { kind: 'float', value: 1.5 }]],
          },
        },
      },
    }

    const html = renderToStaticMarkup(createElement(DataCardCompact, { value, label: 'config' }))

    expect(html).toContain('config')
    expect(html).toContain('rewards')
    expect(html).toContain('rates')
    expect(html).not.toContain('hiddenLeafLabel')
    expect(html).toContain('vc-ref')
    expect(html).toContain('vc-enum')
    expect(html).not.toContain('marker-bullet')
    expect(html).toContain('mobile')
    expect(html).toContain('… +1')
    expect(html).not.toContain('marker-index')
  })

  it('hides the root array label and concrete object item types', () => {
    const value: FieldValue = {
      kind: 'array',
      value: [{
        kind: 'object',
        value: {
          actual_type: 'Reward',
          fields: { amount: { kind: 'int', value: 20n } },
        },
      }],
    }

    const html = renderToStaticMarkup(createElement(DataCardCompact, { value, label: 'drops' }))

    expect(html).not.toContain('drops')
    expect(html).toContain('1.')
    expect(html).not.toContain('Reward')
    expect(html).toContain('20')
    expect(html).not.toContain('amount')
  })
})

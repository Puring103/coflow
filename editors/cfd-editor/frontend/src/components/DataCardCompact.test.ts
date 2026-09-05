import { createElement } from 'react'
import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import {
  collectionObjectDraftForAnnotation,
  dictKeyTemplate,
  DataCardCompact,
  DataCardExpanded,
  EnumDirectSelect,
  MissingValueRepair,
  RefDirectSelect,
} from './DataCard'
import { ObjectDraftHost } from './ObjectDraftHost'
import type { FieldValue } from '../wire'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import { EditorLookupController, type EditorLookupBackend } from '../state/editorLookups'

describe('DataCardCompact complex previews', () => {
  const polymorphicObjectAnnotation: FieldAnnotation = {
    enum_int_value: null,
    declared_type: 'Reward',
    ref_target_type: null,
    enum_type: null,
    enum_is_flag: false,
    nullable: false,
    read_only: false,
    item_annotation: null,
    polymorphic_types: ['CurrencyReward', 'ItemReward'],
    object_type: 'ItemReward',
    field_order: [],
    children: {},
  }

  it('derives empty dictionary key controls from schema annotations', () => {
    expect(dictKeyTemplate({ ...polymorphicObjectAnnotation, declared_type: 'int' })).toEqual({
      kind: 'int',
      value: 0n,
    })
    expect(dictKeyTemplate({
      ...polymorphicObjectAnnotation,
      declared_type: 'Element',
      enum_type: 'Element',
    })).toEqual({
      kind: 'enum',
      value: { enum_name: 'Element', variant: null, value: 0n },
    })
  })

  it('keeps concrete type selection when adding to a populated polymorphic collection', () => {
    expect(collectionObjectDraftForAnnotation(polymorphicObjectAnnotation, false)).toEqual({
      actualType: 'CurrencyReward',
      polymorphicTypes: ['CurrencyReward', 'ItemReward'],
    })
  })

  it('renders a type switch action for a populated polymorphic array item', () => {
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups: {} as never,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{
          name: 'Rewards',
          missing: false,
          annotation: {
            ...polymorphicObjectAnnotation,
            declared_type: '[Reward]',
            object_type: null,
            item_annotation: polymorphicObjectAnnotation,
            children: { '0': polymorphicObjectAnnotation },
          },
          value: {
            kind: 'array' as const,
            value: [{
              kind: 'object' as const,
              value: { actual_type: 'ItemReward', fields: {} },
            }],
          },
        }],
        expandedPaths: new Set(['Rewards']),
        onEdit: () => {},
        onCollectionEdit: () => {},
      }),
    }))

    expect(html).toContain('aria-label="选择具体类型"')
    expect(html).toContain('value="ItemReward"')
  })

  it('shows the concrete type dropdown for a flattened polymorphic field', () => {
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups: {} as never,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{
          name: 'reward',
          missing: false,
          annotation: polymorphicObjectAnnotation,
          value: {
            kind: 'object' as const,
            value: { actual_type: 'ItemReward', fields: {} },
          },
        }],
        flattenSingleComplexField: true,
        onEdit: () => {},
      }),
    }))

    expect(html).toContain('>类型<')
    expect(html).toContain('aria-label="选择具体类型"')
    expect(html).toContain('value="ItemReward"')
  })

  it('renders cached dropdown options immediately after a revision change', async () => {
    const lookups = new EditorLookupController({
      getEnumVariants: async () => [{ name: 'Epic', value: 2n, label: 'Epic', description: null }],
      getRefTargets: async () => [{
        coordinate: { actual_type: 'Item', key: 'sword' },
        file_path: 'data/items.cfd',
      }],
      makeDefaultObject: async () => ({ kind: 'option_none' }),
      createRecordDraft: async (_sessionId, actualType) => ({ actual_type: actualType, fields: [] }),
    } satisfies EditorLookupBackend)
    lookups.adopt({ sessionId: 1, revision: 1 })
    await lookups.loadEnumVariants('Rarity')
    await lookups.loadRefTargets('Item')
    lookups.adopt({ sessionId: 1, revision: 2 })

    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups,
      generationKey: '1:2',
      onOpenReference: () => {},
      children: createElement('div', null,
        createElement(EnumDirectSelect, {
          value: { kind: 'enum', value: { enum_name: 'Rarity', variant: 'Epic', value: 2n } },
          onCommit: () => {},
          variant: 'input',
        }),
        createElement(RefDirectSelect, {
          value: { kind: 'ref', value: '&Item.sword' },
          targetType: 'Item',
          onCommit: () => {},
          variant: 'input',
        }),
      ),
    }))

    expect(html).toContain('value="Epic"')
    expect(html).toContain('value="sword"')
    expect(html).not.toContain('加载中')
    expect(html).not.toContain('disabled')
  })

  it('renders the complete markdown tree while preserving scalar value styles', () => {
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
    expect(html).toContain('iron')
    expect(html).not.toContain('… +1')
    expect(html).not.toContain('marker-index')
  })

  it('does not omit deeply nested values', () => {
    const value: FieldValue = {
      kind: 'array',
      value: [{
        kind: 'object',
        value: {
          actual_type: 'Level1',
          fields: {
            nested: {
              kind: 'object',
              value: {
                actual_type: 'Level2',
                fields: {
                  nested: {
                    kind: 'object',
                    value: {
                      actual_type: 'Level3',
                      fields: { value: { kind: 'string', value: 'fully-visible' } },
                    },
                  },
                },
              },
            },
          },
        },
      }],
    }

    const html = renderToStaticMarkup(createElement(DataCardCompact, { value }))

    expect(html).toContain('fully-visible')
    expect(html).not.toContain('markdown-tree-more')
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

  it('renders every reference in a scalar array', () => {
    const value: FieldValue = {
      kind: 'array',
      value: Array.from({ length: 8 }, (_, index) => ({
        kind: 'ref' as const,
        value: `&GeneConfig.Gene_${index + 1}`,
      })),
    }

    const html = renderToStaticMarkup(createElement(DataCardCompact, { value }))

    for (let index = 1; index <= 8; index += 1) {
      expect(html).toContain(`Gene_${index}`)
    }
    expect(html).toContain('inline-scalar-array')
    expect(html).not.toContain('markdown-tree-more')
  })

  it('applies the referenced type color to scalar previews', () => {
    const value: FieldValue = { kind: 'ref', value: '&ItemConfig.sword' }
    const html = renderToStaticMarkup(createElement(DataCardCompact, {
      value,
      refTargetType: 'ItemConfig',
    }))

    expect(html).toContain('--ref-color:')
    expect(html).toContain('vc-ref')
  })

  it('inlines singleton object collections under a count-only header', () => {
    const fields = [{
      name: 'MatterVariations',
      missing: false,
      annotation: null,
      value: {
        kind: 'array' as const,
        value: [{
          kind: 'object' as const,
          value: {
            actual_type: 'RegionMatterVariationConfig',
            fields: { Matter: { kind: 'string' as const, value: 'Water' } },
          },
        }],
      },
    }]
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups: {} as never,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields,
        actualType: 'MatterConfig',
        expandedPaths: new Set(['MatterVariations']),
        onEdit: () => {},
        onCollectionEdit: () => {},
      }),
    }))

    expect(html).toContain('class="vc-count">1</span>')
    expect(html).toContain('Matter')
    expect(html).toContain('dc-group-body')
    expect(html).toContain('dc-row-actions')
    expect(html).toContain('aria-label="添加元素"')
    expect(html).toContain('title="删除唯一元素"')
    expect(html).not.toContain('#1')
    expect(html).not.toContain('dc-row-item')
    expect(html).not.toContain('元素 1')
    expect(html).not.toContain('>[0]<')
    expect(html).not.toContain('[RegionMatterVariationConfig]')
    expect(html).not.toContain('vc-count">·')
  })

  it('uses one-based index rails for multi-element object collections', () => {
    const html = renderToStaticMarkup(createElement(DataCardExpanded, {
      fields: [{
        name: 'Entries',
        missing: false,
        annotation: null,
        value: {
          kind: 'array' as const,
          value: [1, 2].map(index => ({
            kind: 'object' as const,
            value: {
              actual_type: 'Entry',
              fields: { Value: { kind: 'int' as const, value: BigInt(index) } },
            },
          })),
        },
      }],
      expandedPaths: new Set(['Entries']),
    }))

    expect(html).toContain('dc-array-object-item')
    expect(html).toContain('dc-array-item-index">1</span>')
    expect(html).toContain('dc-array-item-index">2</span>')
    expect(html).not.toContain('#1')
    expect(html).not.toContain('#2')
    expect(html).not.toContain('dc-row-static-group')
  })

  it('uses a narrow one-based index label for scalar collection items', () => {
    const html = renderToStaticMarkup(createElement(DataCardExpanded, {
      fields: [{
        name: 'Values',
        missing: false,
        annotation: null,
        value: {
          kind: 'array' as const,
          value: [
            { kind: 'string' as const, value: 'first' },
            { kind: 'string' as const, value: 'second' },
          ],
        },
      }],
      expandedPaths: new Set(['Values']),
    }))

    expect(html).toContain('dc-row dc-row-field dc-row-item')
    expect(html).toContain('dc-row-label-text">1</span>')
    expect(html).toContain('dc-row-label-text">2</span>')
    expect(html).toContain('first')
    expect(html).toContain('second')
    expect(html).not.toContain('#1')
  })

  it('keeps empty collection state and its add action in the collection header', () => {
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups: {} as never,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{
          name: 'BioRemains',
          missing: false,
          annotation: null,
          value: { kind: 'array' as const, value: [] },
        }],
        flattenSingleComplexField: true,
        onEdit: () => {},
        onCollectionEdit: () => {},
      }),
    }))

    expect(html).toContain('class="vc-count">0</span>')
    expect(html).toContain('BioRemains')
    expect(html).toContain('aria-label="添加元素"')
    expect(html).not.toContain('空数组')
    expect(html).not.toContain('dc-row-empty')
  })

  it('summarizes nested diagnostics without marking ancestor labels as exact errors', () => {
    const fields = [{
      name: 'MatterVariations',
      missing: false,
      annotation: null,
      value: {
        kind: 'array' as const,
        value: [{
          kind: 'object' as const,
          value: {
            actual_type: 'RegionMatterVariationConfig',
            fields: { PrefabId: { kind: 'string' as const, value: '' } },
          },
        }],
      },
    }]
    const html = renderToStaticMarkup(createElement(DataCardExpanded, {
      fields,
      actualType: 'MatterConfig',
      expandedPaths: new Set(['MatterVariations', 'MatterVariations[0]']),
      diagnostics: [{
        severity: 'error',
        field_path: 'MatterVariations[0].PrefabId',
        message: 'PrefabId is required',
      }],
    }))

    expect(html).toContain('dc-row-diag-summary')
    expect(html).toContain('dc-row-diag-exact')
    expect(html).toContain('查看错误诊断')
  })

  it('keeps deeply nested inspector objects in the expanded form layout', () => {
    let nested: FieldValue = { kind: 'string', value: 'deep-leaf' }
    for (let depth = 7; depth >= 1; depth -= 1) {
      nested = {
        kind: 'object',
        value: {
          actual_type: `Level${depth}`,
          fields: { [`level${depth}`]: nested },
        },
      }
    }
    const expandedPaths = new Set<string>(['root'])
    let path = 'root'
    for (let depth = 1; depth <= 6; depth += 1) {
      path += `.level${depth}`
      expandedPaths.add(path)
    }
    const html = renderToStaticMarkup(createElement(DataCardExpanded, {
      fields: [{ name: 'root', annotation: null, value: nested, missing: false }],
      expandedPaths,
    }))

    expect(html).toContain('deep-leaf')
    expect(html).toContain(`data-field-path="${path}.level7"`)
    expect(html).not.toContain('markdown-value-tree')
  })

})

describe('missing field repair', () => {
  it('renders omitted object fields from schema annotations', () => {
    const lookups = new EditorLookupController({
      getEnumVariants: async () => [],
      getRefTargets: async () => [],
      makeDefaultObject: async () => ({ kind: 'option_none' }),
      createRecordDraft: async (_sessionId, actualType) => ({ actual_type: actualType, fields: [] }),
    } satisfies EditorLookupBackend)
    lookups.adopt({ sessionId: 1, revision: 1 })
    const childBase = {
      enum_int_value: null,
      enum_type: null,
      enum_is_flag: false,
      nullable: false,
      read_only: false,
      item_annotation: null,
      field_order: [],
      children: {},
    }
    const annotation = {
      ...childBase,
      declared_type: 'Holder',
      ref_target_type: null,
      polymorphic_types: [],
      object_type: 'Holder',
      field_order: ['target', 'effect'],
      children: {
        target: {
          ...childBase,
          declared_type: '&Item',
          ref_target_type: 'Item',
          polymorphic_types: [],
          object_type: null,
        },
        effect: {
          ...childBase,
          declared_type: 'Effect',
          ref_target_type: null,
          polymorphic_types: ['Damage', 'Heal'],
          object_type: 'Effect',
        },
      },
    } satisfies FieldAnnotation
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{
          name: 'holder',
          value: { kind: 'object', value: { actual_type: 'Holder', fields: {} } },
          missing: false,
          annotation,
        }],
        expandedPaths: new Set(['holder']),
        onEdit: () => {},
      }),
    }))

    expect(html).toContain('aria-label="选择引用"')
    expect(html).toContain('aria-label="创建值"')
    expect(html).toContain('target')
    expect(html).toContain('effect')
  })

  it('renders a missing required reference as a reference selector', () => {
    const lookups = new EditorLookupController({
      getEnumVariants: async () => [],
      getRefTargets: async () => [],
      makeDefaultObject: async () => ({ kind: 'option_none' }),
      createRecordDraft: async (_sessionId, actualType) => ({ actual_type: actualType, fields: [] }),
    } satisfies EditorLookupBackend)
    lookups.adopt({ sessionId: 1, revision: 1 })
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{
          name: 'target',
          value: { kind: 'option_none' },
          missing: true,
          annotation: {
            enum_int_value: null,
            declared_type: '&Item',
            ref_target_type: 'Item',
            enum_type: null,
            enum_is_flag: false,
            nullable: false,
            read_only: false,
            item_annotation: null,
            polymorphic_types: [],
            object_type: null,
            field_order: [],
            children: {},
          },
        }],
        onEdit: () => {},
      }),
    }))

    expect(html).toContain('aria-label="选择引用"')
    expect(html).not.toContain('没有可用的默认值')
  })

  it('renders an explicit repair action for a generated default', () => {
    const html = renderToStaticMarkup(createElement(MissingValueRepair, {
      value: { kind: 'int', value: 0n },
      onRepair: () => {},
    }))

    expect(html).toContain('Missing')
    expect(html).toContain('修复')
    expect(html).not.toContain('disabled=""')
  })

  it('disables repair when no valid default can be generated', () => {
    const html = renderToStaticMarkup(createElement(MissingValueRepair, {
      value: { kind: 'option_none' },
      onRepair: () => {},
    }))

    expect(html).toContain('没有可用的默认值')
    expect(html).toContain('disabled=""')
  })
})

describe('Option value controls', () => {
  const annotation: FieldAnnotation = {
    enum_int_value: null,
    declared_type: 'Option<int>',
    ref_target_type: null,
    enum_type: null,
    enum_is_flag: false,
    nullable: true,
    read_only: false,
    item_annotation: null,
    polymorphic_types: [],
    object_type: null,
    field_order: [],
    children: {},
  }

  function render(value: FieldValue) {
    return renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups: {} as never,
      generationKey: 'test',
      onOpenReference: () => {},
      children: createElement(DataCardExpanded, {
        fields: [{ name: 'value', value, missing: false, annotation }],
        onEdit: () => {},
      }),
    }))
  }

  it('shows a plus action for None', () => {
    const html = render({ kind: 'option_none' })

    expect(html).toContain('aria-label="创建值"')
    expect(html).not.toContain('aria-label="清除为 None"')
  })

  it('shows a clear action for Some', () => {
    const html = render({
      kind: 'option_some',
      value: { kind: 'int', value: 1n },
    })

    expect(html).toContain('aria-label="清除为 None"')
    expect(html).not.toContain('aria-label="创建值"')
  })

  it('renders controls for each nested Option layer', () => {
    const nestedAnnotation = {
      ...annotation,
      declared_type: 'Option<Option<int>>',
    }
    const renderNested = (value: FieldValue) => renderToStaticMarkup(createElement(
      ObjectDraftHost,
      {
        lookups: {} as never,
        generationKey: 'test',
        onOpenReference: () => {},
        children: createElement(DataCardExpanded, {
          fields: [{ name: 'value', value, missing: false, annotation: nestedAnnotation }],
          onEdit: () => {},
        }),
      },
    ))

    const someNone = renderNested({
      kind: 'option_some',
      value: { kind: 'option_none' },
    })
    expect(someNone.match(/aria-label="清除为 None"/g)).toHaveLength(1)
    expect(someNone.match(/aria-label="创建值"/g)).toHaveLength(1)

    const someSome = renderNested({
      kind: 'option_some',
      value: { kind: 'option_some', value: { kind: 'int', value: 1n } },
    })
    expect(someSome.match(/aria-label="清除为 None"/g)).toHaveLength(2)
    expect(someSome).not.toContain('aria-label="创建值"')
  })
})

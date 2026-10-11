import { describe, expect, it } from 'vitest'
import { fieldMetadataTitle } from './fieldMetadata'

describe('fieldMetadataTitle', () => {
  it('shows the actual field name', () => {
    expect(fieldMetadataTitle('attack_power')).toBe('实际名称：attack_power')
  })

  it('appends the description when present', () => {
    expect(fieldMetadataTitle('attack_power', '角色的基础攻击力'))
      .toBe('实际名称：attack_power\n描述：角色的基础攻击力')
  })
})

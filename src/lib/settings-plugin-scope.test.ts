import { describe, it, expect } from 'vitest'
import { getPluginScopedKey } from './settings.svelte'

describe('dotted plugin ids', () => {
  it('documents why getPluginScopedKey must not be used for v2 ids', () => {
    // 这不是「期望的行为」,是钉住已知陷阱:fq key 按第一个点切分,
    // v2 id 一律含点,所以新代码用 getPluginScopedValue(pluginId, key)。
    expect(getPluginScopedKey('notemd.power-mode.config')).toBeUndefined()
  })
})

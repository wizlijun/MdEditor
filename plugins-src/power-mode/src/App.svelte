<script lang="ts">
  import '../../../src/styles/ui-foundation.css'
  import { onMount, onDestroy } from 'svelte'
  import { bridge, loadPowerMode, savePowerMode, type SurfaceEntry } from './lib/bridge'
  import { normalizeConfig, DEFAULT_CONFIG, PRESET_IDS } from './lib/config'
  import type { PowerModeConfig, PresetId } from './lib/types'
  import { loadKit, type KitEditor } from './lib/editor-kit'
  import { setLocale, t, type MessageKey } from './lib/strings'

  const locale = bridge().locale
  setLocale(locale)

  let cfg = $state<PowerModeConfig>(structuredClone(DEFAULT_CONFIG))
  let surfaces = $state<SurfaceEntry[]>([])
  let demoHost = $state<HTMLDivElement | null>(null)
  let kit: KitEditor | null = null
  let kitFailed = $state(false)
  let saveTimer: ReturnType<typeof setTimeout> | undefined
  let error = $state<string | null>(null)
  let saveError = $state(false)

  function surfaceLabel(s: SurfaceEntry): string {
    return s.names[locale] ?? s.names[locale.split('-')[0] ?? ''] ?? s.name
  }

  /** 未列出的插件窗口默认开,'main' 默认关 —— 与引擎的 isSurfaceEnabled 同规则。 */
  function surfaceOn(id: string): boolean {
    const v = cfg.surfaces[id]
    return typeof v === 'boolean' ? v : id !== 'main'
  }

  /** 每次改动:实操区立刻跟上,落盘 debounce 300 ms。 */
  function touched(): void {
    kit?.setPowerMode($state.snapshot(cfg))
    if (saveTimer !== undefined) clearTimeout(saveTimer)
    saveTimer = setTimeout(async () => {
      try {
        await savePowerMode($state.snapshot(cfg))
        error = null
        saveError = false
      } catch (e) {
        error = `${t('saveFailed')}: ${String(e)}`
        saveError = true
      }
    }, 300)
  }

  function setSurface(id: string, on: boolean): void {
    cfg.surfaces = { ...cfg.surfaces, [id]: on }
    touched()
  }

  const presetLabel = (id: PresetId): string => t(`preset.${id}` as MessageKey)

  // 强度是三档而非连续滑块:一档同时定 intensity 与 recoverTime,选中态按
  // intensity 匹配(旧配置里的自定义值不匹配任何档,segmented 就都不高亮)。
  const SHAKE_LEVELS = [
    { key: 'light',  intensity: 3,  recoverTime: 500 },
    { key: 'medium', intensity: 6,  recoverTime: 800 },
    { key: 'heavy',  intensity: 10, recoverTime: 1200 },
  ] as const

  function setShakeLevel(l: (typeof SHAKE_LEVELS)[number]): void {
    cfg.shake = { ...cfg.shake, intensity: l.intensity, recoverTime: l.recoverTime }
    touched()
  }

  // 超时同样改成三档。
  const COMBO_TIMEOUTS = [
    { key: 'short',  seconds: 5 },
    { key: 'medium', seconds: 10 },
    { key: 'long',   seconds: 20 },
  ] as const

  function setComboTimeout(seconds: number): void {
    cfg.combo = { ...cfg.combo, timeout: seconds }
    touched()
  }

  onMount(async () => {
    try {
      const payload = await loadPowerMode()
      cfg = normalizeConfig(payload.config ?? {})
      surfaces = payload.surfaces
    } catch (e) {
      error = String(e)
    }
    if (!demoHost) return
    try {
      const mount = await loadKit()
      kit = await mount(demoHost, {
        initialMarkdown: t('demo.sample'),
        mode: 'rich',
        // 显式给值 = 实操区自管,不受上面的生效面开关影响。
        powerMode: $state.snapshot(cfg),
      })
    } catch {
      kitFailed = true
    }
  })

  onDestroy(() => {
    if (saveTimer !== undefined) clearTimeout(saveTimer)
    kit?.destroy()
  })
</script>

<main class="ui-surface">
  <h1>{t('title')}</h1>

  {#if error}
    <div class="error" role="alert">
      <p>{error}</p>
      {#if saveError}<button type="button" onclick={touched}>{t('retry')}</button>{/if}
    </div>
  {/if}

  <section>
    <h2>{t('surfaces.section')}</h2>
    <label class="row">
      <input type="checkbox" checked={surfaceOn('main')}
             onchange={(e) => setSurface('main', e.currentTarget.checked)} />
      <span>{t('surfaces.main')}</span>
    </label>
    {#each surfaces as s (s.id)}
      <label class="row">
        <input type="checkbox" checked={surfaceOn(s.id)}
               onchange={(e) => setSurface(s.id, e.currentTarget.checked)} />
        <span>{surfaceLabel(s)}</span>
      </label>
    {/each}
    <p class="hint">{t('surfaces.hint')}</p>
  </section>

  <section>
    <h2>{t('effects.section')}</h2>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.explosion.enable} onchange={touched} />
      <span>{t('explosion.enable')}</span>
    </label>
    <label class="row indent">
      <span>{t('explosion.preset')}</span>
      <select bind:value={cfg.explosion.presetId} onchange={touched} disabled={!cfg.explosion.enable}>
        {#each PRESET_IDS as id (id)}
          <option value={id}>{presetLabel(id)}</option>
        {/each}
      </select>
    </label>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.shake.enable} onchange={touched} />
      <span>{t('shake.enable')}</span>
    </label>
    <div class="row indent">
      <span>{t('shake.intensity')}</span>
      <div class="seg" role="group" aria-label={t('shake.intensity')}>
        {#each SHAKE_LEVELS as l (l.key)}
          <button type="button" class="seg-btn" class:on={cfg.shake.intensity === l.intensity}
                  aria-pressed={cfg.shake.intensity === l.intensity} disabled={!cfg.shake.enable} onclick={() => setShakeLevel(l)}>
            {t(`shake.level.${l.key}` as MessageKey)}
          </button>
        {/each}
      </div>
    </div>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.combo.enable} onchange={touched} />
      <span>{t('combo.enable')}</span>
    </label>
    <div class="row indent">
      <span>{t('combo.timeout')}</span>
      <div class="seg" role="group" aria-label={t('combo.timeout')}>
        {#each COMBO_TIMEOUTS as o (o.key)}
          <button type="button" class="seg-btn" class:on={cfg.combo.timeout === o.seconds}
                  aria-pressed={cfg.combo.timeout === o.seconds} disabled={!cfg.combo.enable} onclick={() => setComboTimeout(o.seconds)}>
            {t(`combo.timeout.${o.key}` as MessageKey)}
          </button>
        {/each}
      </div>
    </div>
    <label class="row indent">
      <input type="checkbox" bind:checked={cfg.combo.showExclamation}
             onchange={touched} disabled={!cfg.combo.enable} />
      <span>{t('combo.showExclamation')}</span>
    </label>
    <label class="row indent">
      <input type="checkbox" bind:checked={cfg.combo.precisionInput}
             onchange={touched} disabled={!cfg.combo.enable} />
      <span>{t('combo.precisionInput')}</span>
    </label>
    <p class="hint indent">{t('combo.precisionInput.hint')}</p>
  </section>

  <section class="demo">
    <h2>{t('demo.section')}</h2>
    <p class="hint">{t('demo.hint')}</p>
    {#if kitFailed}
      <p class="hint">{t('demo.unavailable')}</p>
    {:else}
      <div class="demo-host" bind:this={demoHost}></div>
    {/if}
  </section>
</main>

<style>
  /* 独立 Tauri 窗口须自声明 color-scheme,否则系统深色下 Canvas 系统色卡浅。 */
  :global(html) { color-scheme: light dark; }
  :global(body) { margin: 0; font: 13px/1.5 -apple-system, system-ui, sans-serif; }

  main {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 18px 20px;
    min-height: 100vh;
    box-sizing: border-box;
  }
  h1 { font-size: 17px; margin: 0; }
  h2 { font-size: 13px; color: var(--ui-secondary); margin: 0 0 8px; }
  section { display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 5px 0; }
  .row.indent { padding-left: 22px; }
  .row span { min-width: 0; overflow-wrap: anywhere; }

  /* 分段选择器:三档强度 / 三档超时。 */
  .seg { display: inline-flex; margin-left: auto; border-radius: 6px; overflow: hidden;
         border: 1px solid color-mix(in srgb, currentColor 22%, transparent); }
  .seg-btn {
    appearance: none; border: 0; background: transparent; color: inherit;
    font: inherit; padding: 3px 12px; cursor: pointer;
    border-left: 1px solid color-mix(in srgb, currentColor 22%, transparent);
  }
  .seg-btn:first-child { border-left: 0; }
  .seg-btn.on { background: color-mix(in srgb, currentColor 16%, transparent); font-weight: 600; }
  .seg-btn:disabled { opacity: .4; cursor: default; }

  .hint { margin: 4px 0 0; color: var(--ui-secondary); font-size: 12px; }
  .hint.indent { padding-left: 22px; }
  .error { color: var(--ui-danger); margin: 0; overflow-wrap: anywhere; }
  .error p { margin: 0 0 6px; }

  /* Kit 要求容器有确定高度:content-sized 容器下 source 模式会塌成 0。 */
  .demo { flex: 1; min-height: 0; }
  .demo-host {
    flex: 1 0 180px;
    min-height: 180px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 6px;
    overflow: hidden;
  }
</style>

<script lang="ts">
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
      } catch (e) {
        error = `${t('saveFailed')}: ${String(e)}`
      }
    }, 300)
  }

  function setSurface(id: string, on: boolean): void {
    cfg.surfaces = { ...cfg.surfaces, [id]: on }
    touched()
  }

  const presetLabel = (id: PresetId): string => t(`preset.${id}` as MessageKey)

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

<main>
  <h1>{t('title')}</h1>

  {#if error}<p class="error">{error}</p>{/if}

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
    <label class="row indent">
      <span>{t('shake.intensity')}</span>
      <input type="range" min="1" max="20" step="1" bind:value={cfg.shake.intensity}
             oninput={touched} disabled={!cfg.shake.enable} />
      <output>{cfg.shake.intensity} px</output>
    </label>
    <label class="row indent">
      <span>{t('shake.recoverTime')}</span>
      <input type="range" min="100" max="2000" step="50" bind:value={cfg.shake.recoverTime}
             oninput={touched} disabled={!cfg.shake.enable} />
      <output>{cfg.shake.recoverTime} ms</output>
    </label>

    <label class="row">
      <input type="checkbox" bind:checked={cfg.combo.enable} onchange={touched} />
      <span>{t('combo.enable')}</span>
    </label>
    <label class="row indent">
      <span>{t('combo.timeout')}</span>
      <input type="range" min="2" max="30" step="1" bind:value={cfg.combo.timeout}
             oninput={touched} disabled={!cfg.combo.enable} />
      <output>{cfg.combo.timeout} s</output>
    </label>
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
    height: 100vh;
    box-sizing: border-box;
  }
  h1 { font-size: 17px; margin: 0; }
  h2 { font-size: 12px; text-transform: uppercase; letter-spacing: .06em; opacity: .6; margin: 0 0 8px; }
  section { display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 8px; padding: 3px 0; }
  .row.indent { padding-left: 22px; }
  .row span { flex: 0 0 auto; }
  .row input[type='range'] { flex: 1; }
  output { min-width: 56px; text-align: right; opacity: .7; font-variant-numeric: tabular-nums; }
  .hint { margin: 4px 0 0; opacity: .55; font-size: 12px; }
  .hint.indent { padding-left: 22px; }
  .error { color: #d33; margin: 0; }

  /* Kit 要求容器有确定高度:content-sized 容器下 source 模式会塌成 0。 */
  .demo { flex: 1; min-height: 0; }
  .demo-host {
    flex: 1;
    min-height: 180px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 6px;
    overflow: hidden;
  }
</style>

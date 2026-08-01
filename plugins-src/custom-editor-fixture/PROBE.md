# Custom-Editor Iframe Mechanism — GUI Probe Checklist

**Purpose.** Validate that the `plugin://` iframe embedding mechanism (子项目④) is
end-to-end viable before the `base` plugin migrates to a custom editor tab. This
fixture (`notemd.cef-fixture`) maps `.cef` files to a minimal textarea editor
served under `plugin://notemd.cef-fixture/editor.html`.

**Pass criteria.** If steps (a)–(e) all pass, tab-embedded custom editors are
viable and base can migrate as designed. If focus-in-tab or Cmd+S (steps d, f, g)
fail, base falls back to a detached window editor as noted in the plan (Task 3).

---

## Setup

### Step 0 — Install the fixture plugin

```bash
# From the repo root (or the isolated worktree):
bash scripts/dev-install-plugin.sh cef
```

This builds `plugins-src/custom-editor-fixture/dist/editor.html` and installs it
under `~/Library/Application Support/net.notemd.app/plugins/notemd.cef-fixture/`.

### Step 1 — Launch the app

```bash
pnpm tauri dev
```

---

## Creating a .cef file (Step 0 decision: host-side create handler)

The fixture's "New .cef fixture" command is handled by the host (not the plugin
process — this is a ui-only plugin with no binary). When you click the menu item:

1. A **save dialog** appears asking where to create the file.
2. The host writes an empty file at the chosen path (e.g. `~/Desktop/test.cef`).
3. The host opens that file, routing it through the custom-editor registry to the
   `notemd.cef-fixture` plugin's `editor.html` entry.

**Alternative:** You can also create an empty file manually and open it via
File ▸ Open (or drag it onto the window). Any file with the `.cef` extension
will be routed to the fixture editor.

---

## Probe Steps

### (a) Tab renders an iframe with a textarea

- Click **File ▸ New .cef fixture** (or open any `.cef` file).
- **Expected:** A new tab opens. The tab body shows a dark/light themed frame
  with a monospace `<textarea>` and a toolbar labelling it
  "CEF Fixture — Custom Editor Probe". There is no ProseMirror or source-view
  — only the iframe.
- **Fail signal:** Tab body is blank, shows an error banner, or shows the
  Markdown editor instead of the iframe.

### (b) "cef editor ready" toast appears

- Within ~1 second of the tab opening, a toast notification with level `info`
  and text **"cef editor ready"** appears in the app's toast area.
- **Expected:** This toast proves the full fetch-RPC bridge path:
  `window.notemd.request('host.toast', …)` → `plugin:// /__rpc__` POST →
  Rust `ui_rpc::dispatch` → `host.toast` capability → frontend toast queue.
- **Fail signal:** No toast. Check the devtools console (View ▸ Developer Tools)
  for `[cef-fixture] window.notemd not available` or network errors to
  `plugin://notemd.cef-fixture/__rpc__`.

### (c) Typing marks the tab dirty

- Click inside the textarea and type a few characters (e.g. "hello cef").
- **Expected:** The tab title in the tab bar gains a dirty indicator (a dot `•`
  or the title changes to `test.cef •`). The status area in the editor's toolbar
  changes to "— modified".
- **Fail signal:** No dirty indicator. The `change` postMessage from the iframe
  is not reaching the host's `handleCustomEditorMessage` / `setContent`.

### (d) Cmd+S saves and clears dirty

- With the tab dirty (step c), press **Cmd+S**.
- **Expected:** The dirty indicator disappears. The file on disk contains the
  text you typed. Verify with Terminal:
  ```bash
  cat ~/Desktop/test.cef
  ```
- **Fail signal:** Cmd+S does nothing visible. The dirty indicator stays. The
  file on disk is still empty (or the previous content).

### (e) Close and reopen — content persists

- Close the tab (Cmd+W or the tab's × button). If a "discard changes" prompt
  appears and the file is saved, dismiss it (or save first via Cmd+S).
- Reopen the same `.cef` file: **File ▸ Open Recent** (it should appear there)
  or File ▸ Open → navigate to the file.
- **Expected:** The tab reopens with the content you saved in step (d). The
  textarea is populated immediately after the `custom_editor.open` message
  arrives from the host.
- **Fail signal:** Tab opens empty even though the file on disk has content.
  Check that `CustomEditorIframe.svelte`'s `onLoad → win.postMessage(open, …)`
  is firing and that the textarea's `message` listener sets `editor.value`.

### (f) Cmd+A / Cmd+C / Cmd+Z inside the textarea

- Click inside the textarea (ensure it has focus — you may need to click once
  first, then Cmd+A).
- **Cmd+A** — selects all text in the textarea.
- **Cmd+C** — copies selected text to the clipboard.
- **Cmd+Z** — undoes the last typed character (native textarea undo).
- **Expected:** All three behave as they would in any system textarea. The host's
  own Cmd+A / Cmd+C / Cmd+Z handlers do NOT fire while the iframe has focus.
- **Fail signal:** Cmd+A triggers the host's "Select All" (selects nothing in
  the textarea), Cmd+Z does nothing, or the key goes to the host editor instead
  of the textarea.
- **Note:** If these fail but (a)–(e) pass, base can still migrate with a known
  caveat: keyboard shortcuts inside the iframe compete with host bindings.
  Document the failure here and decide in Task 4.

### (g) Focus behaviour

- Click a different tab, then click back to the `.cef` tab.
- Click directly on the textarea.
- Type a character.
- **Expected:** The character appears in the textarea, not somewhere else. The
  tab's dirty indicator updates (step c behaviour).
- Also verify: clicking the tab bar itself (not the body) does NOT move focus
  into the textarea until you explicitly click the textarea body.
- **Fail signal:** Focus is "trapped" in the host, characters go to the main
  editor, or clicking the textarea does not give it input focus.

### (h) Scroll a long document

- Paste (or type) enough lines to make the textarea overflow vertically (or
  paste a long string).
- **Expected:** The textarea scrolls smoothly inside the iframe. The host
  window does not scroll. The iframe itself does not overflow the tab body.
- **Fail signal:** Scroll events escape the iframe and scroll the host window,
  or the textarea content is clipped with no scrollbar.

### (i) Theme / colour-scheme follows the app

- Toggle the app's theme between light and dark (Settings ▸ Appearance or the
  theme picker in the sidebar).
- **Expected:** The iframe's background (`Canvas`) and text (`CanvasText`)
  adapt. The toolbar text and the textarea update to match.
- **Note:** The fixture uses CSS system colours (`Canvas`, `CanvasText`,
  `color-scheme: light dark`) — it does NOT read the app's `data-theme`
  attribute, so this only tests basic OS-level color-scheme propagation into
  the iframe, not full theme sync. Note any glitch.

---

## Pass / Fail Rubric

| Step | What it tests | Required for "viable"? |
|------|--------------|------------------------|
| (a) iframe renders | CustomEditorIframe routing, protocol.rs asset serving | YES — blocker |
| (b) toast from iframe | fetch-RPC bridge inside iframe (inject_bridge) | YES — blocker |
| (c) dirty on type | `change` postMessage → setContent | YES — blocker |
| (d) Cmd+S saves | host save path for custom tabs | YES — blocker |
| (e) reopen persists | `custom_editor.open` content population | YES — blocker |
| (f) Cmd+A/C/Z work | native textarea keyboard shortcuts in iframe | Strongly desired; caveat if fail |
| (g) focus | iframe click → textarea gets input focus | Strongly desired; caveat if fail |
| (h) scroll | overflow within iframe | Minor; note if broken |
| (i) colour-scheme | system colour propagation | Minor; note if broken |

**PASS:** (a)–(e) all green → base migrates as a custom-editor tab (Task 4).

**PARTIAL:** (a)–(e) pass, (f)/(g) fail → base migrates but ships with a
keyboard / focus caveat note for the user. Document and decide in Task 4.

**FAIL:** Any of (a)–(e) fail → investigate and fix the mechanism before
migrating base. Consult `src/components/CustomEditorIframe.svelte`,
`src-tauri/src/plugin_runtime/protocol.rs`, and `src/lib/plugins/custom-editors.ts`.

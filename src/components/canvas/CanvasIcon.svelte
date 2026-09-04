<script lang="ts">
  export type CanvasIconName =
    | 'select' | 'pan' | 'lasso' | 'lock' | 'unlock'
    | 'text' | 'file' | 'link' | 'group' | 'frame'
    | 'copy' | 'cut' | 'paste' | 'undo' | 'redo' | 'trash'
    | 'front' | 'back' | 'align-left' | 'align-center-h' | 'align-right'
    | 'align-top' | 'align-center-v' | 'align-bottom'
    | 'distribute-h' | 'distribute-v' | 'spread'
    | 'arrow-start' | 'arrow-end' | 'fit' | 'image' | 'ungroup'
    | 'edit' | 'palette'

  let { name, size = 18 }: { name: CanvasIconName; size?: number } = $props()

  const paths: Record<CanvasIconName, readonly string[]> = {
    select: ['M4 3l7.5 17 2.2-6.1 6.3-2.2L4 3z'],
    pan: ['M7 11V6.5a1.5 1.5 0 0 1 3 0V10', 'M10 10V4.5a1.5 1.5 0 0 1 3 0V10', 'M13 10V6a1.5 1.5 0 0 1 3 0v5', 'M16 10.5V8a1.5 1.5 0 0 1 3 0v5.5c0 4.2-2.8 7.5-7 7.5h-1.2a6 6 0 0 1-4.7-2.3L3.5 15.4a1.6 1.6 0 0 1 2.3-2.2L8 15'],
    lasso: ['M7.2 19.3C4.1 18.5 2 16.7 2 14.5 2 11.5 6.5 9 12 9s10 2.5 10 5.5S17.5 20 12 20c-1.1 0-2.1-.1-3-.3', 'M7.5 22a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z'],
    lock: ['M6 10V7a6 6 0 0 1 12 0v3', 'M5 10h14v11H5z', 'M12 14v3'],
    unlock: ['M6 10V7a6 6 0 0 1 10.8-3.6', 'M5 10h14v11H5z', 'M12 14v3'],
    text: ['M4 5V3h16v2', 'M9 21h6', 'M12 3v18'],
    file: ['M6 2h8l4 4v16H6z', 'M14 2v5h5', 'M9 13h6', 'M9 17h6'],
    link: ['M10 13a5 5 0 0 0 7.1.1l2-2a5 5 0 0 0-7.1-7.1l-1.1 1.1', 'M14 11a5 5 0 0 0-7.1-.1l-2 2A5 5 0 0 0 12 20l1.1-1.1'],
    group: ['M4 4h6v6H4z', 'M14 4h6v6h-6z', 'M4 14h6v6H4z', 'M14 14h6v6h-6z'],
    frame: ['M4 9V4h5', 'M15 4h5v5', 'M20 15v5h-5', 'M9 20H4v-5'],
    copy: ['M8 8h12v12H8z', 'M4 16V4h12'],
    cut: ['M4 4l16 16', 'M20 4L4 20', 'M6 6a2 2 0 1 1-4 0 2 2 0 0 1 4 0z', 'M22 18a2 2 0 1 1-4 0 2 2 0 0 1 4 0z'],
    paste: ['M9 5h6v3H9z', 'M8 6H5v16h14V6h-3', 'M9 13h6', 'M9 17h6'],
    undo: ['M9 7l-5 5 5 5', 'M20 17a8 8 0 0 0-8-8H4'],
    redo: ['M15 7l5 5-5 5', 'M4 17a8 8 0 0 1 8-8h8'],
    trash: ['M4 7h16', 'M9 3h6l1 4H8z', 'M7 7l1 14h8l1-14', 'M10 11v6', 'M14 11v6'],
    front: ['M8 8h11v11H8z', 'M5 16V5h11'],
    back: ['M5 5h11v11H5z', 'M8 19h11V8'],
    'align-left': ['M5 3v18', 'M9 7h10', 'M9 12h7', 'M9 17h10'],
    'align-center-h': ['M12 3v18', 'M5 7h14', 'M7 12h10', 'M5 17h14'],
    'align-right': ['M19 3v18', 'M5 7h10', 'M8 12h7', 'M5 17h10'],
    'align-top': ['M3 5h18', 'M7 9v10', 'M12 9v7', 'M17 9v10'],
    'align-center-v': ['M3 12h18', 'M7 5v14', 'M12 7v10', 'M17 5v14'],
    'align-bottom': ['M3 19h18', 'M7 5v10', 'M12 8v7', 'M17 5v10'],
    'distribute-h': ['M4 3v18', 'M20 3v18', 'M8 8h3v8H8z', 'M14 8h3v8h-3z'],
    'distribute-v': ['M3 4h18', 'M3 20h18', 'M8 8h8v3H8z', 'M8 14h8v3H8z'],
    spread: ['M8 8H4V4', 'M16 8h4V4', 'M8 16H4v4', 'M16 16h4v4', 'M4 4l5 5', 'M20 4l-5 5', 'M4 20l5-5', 'M20 20l-5-5'],
    'arrow-start': ['M5 12h14', 'M9 8l-4 4 4 4'],
    'arrow-end': ['M5 12h14', 'M15 8l4 4-4 4'],
    fit: ['M8 3H3v5', 'M16 3h5v5', 'M8 21H3v-5', 'M16 21h5v-5', 'M8 8h8v8H8z'],
    image: ['M4 4h16v16H4z', 'M8.5 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z', 'M4 17l5-5 3 3 2-2 6 6'],
    ungroup: ['M10 4H4v6', 'M14 4h6v6', 'M10 20H4v-6', 'M14 20h6v-6', 'M9 9l6 6', 'M15 9l-6 6'],
    edit: ['M4 20h4L19 9l-4-4L4 16z', 'M13.5 6.5l4 4'],
    palette: ['M12 3a9 9 0 0 0 0 18h1.5a1.5 1.5 0 0 0 0 0-3H12a2 2 0 0 1 0-4h3a6 6 0 0 0 0-3-13z', 'M7.5 10h.01', 'M10 7h.01', 'M14 7h.01', 'M16.5 10h.01'],
  }
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each paths[name] as path}
    <path d={path}></path>
  {/each}
</svg>

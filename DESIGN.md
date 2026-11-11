# Subtext

Subtext is a resident notes drawer for Wayland. One process stays mapped for the life of the session. A hotkey slides a text plate in from a screen edge, and the same hotkey, Escape, or a click outside slides it away. It is for scratch notes and short code, not a project editor.

The stack is Rust 2024, iced 0.14 with the tiny-skia renderer, and iced_layershell. Text is shaped by cosmic-text and highlighted with the syntect highlighter iced already ships. State is JSON through serde. File dialogs use rfd. There is no async runtime: the hotkey client is a short-lived process that writes one line to a Unix socket.

## Process

`subtext` with no arguments is the daemon. It binds `$XDG_RUNTIME_DIR/subtext.sock` and refuses to start when that socket is already taken. `subtext toggle`, `show`, and `hide` write that word to the socket and exit. If nothing is listening, the client starts the daemon and sends the command again.

The daemon opens the layer surface immediately and leaves it parked off the anchor edge, so the first toggle is a slide rather than a first map.

## Surfaces

The drawer is one layer-shell surface anchored to all four edges with a requested size of 0×0, so it covers the output. The plate is a widget inside that surface. Its input region is the on-screen part of the plate. A parked drawer adds nothing to the region, which makes the rest of the output click-through. An empty region is click-through. A null region would be the whole output.

While the drawer is open its layer is `Overlay`, except while Settings is open, when the drawer drops to `Top` so the settings surface can sit above it. Both drop to `Bottom` while a file dialog is open, because an ordinary xdg dialog cannot cover `Top` or `Overlay`. Keyboard interactivity is exclusive while the drawer is open, on-demand while Settings is open, and none while a dialog is open or the drawer is parked.

Settings is a second layer surface, centered, namespace `subtext-settings`, on the overlay layer. It is not a page inside the drawer. Closing it returns focus to the editor. Escape closes Settings first, then a scratch-close prompt, then the find bar, and then hides the drawer.

The plate slides by moving its layout bounds. The surface is not resized to animate, and `org_kde_kwin_slide` is not used. KWin's slide effect blurs the whole fullscreen surface and does not repaint the panel when the animation ends. Travel is the plate's own span along the anchor, over 150ms with an ease-out cubic. The first show after launch waits out KWin's generic map animation before that slide starts. Buffer alpha stays opaque: a transparent plate would reveal the blur at full strength instead of fading it.

## Blur and opacity

Opacity is the alpha of the plate, from 0.2 to 1.0. The editor background is clear so the plate shows through. The margin outside the plate stays fully clear.

Blur is `org_kde_kwin_blur`. The protocol has no radius; that stays a KWin setting. The blur region is the on-screen intersection of the plate. KWin reads the region when the blur object is attached, so a changed rectangle is a new object with the region already set. An empty region means the whole surface, so the drawer never sends one. When the plate is fully past the edge, the drawer blur is detached and the surface is not committed from that path. The following iced frame publishes the change. Settings blurs its whole window. Until the drawer rectangle is known, the first paint stays opaque so the screen is not one sheet of frost.

## Text

The size control is points by default. iced draws logical pixels, and one point is 96/72 of a pixel, which is what a terminal means by 16pt. The pt/px toggle converts the number so the glyphs stay the same size, then rounds to the nearest whole number in that unit. Points run from 8 to 36. Pixels run from 10 to 48.

The font list is System, then every installed family whose font file says it is monospace. System is iced's generic monospace. Names are the family names from the font files. An unknown saved name falls back to System.

Hinting is on by default. Off sets cosmic-text's `DISABLE_HINTING` flag and drops the quarter-pixel subpixel bin, so the outline is unhinted and sits on a whole pixel. On is swash's normal smooth hinter. A bitmap strike at a matching size is sharp either way, because that path never goes through the hinter.

For a monospace face, layout then replaces the linear advance with the hinted advance of the space glyph. That is the cell a terminal uses. At 16pt, 3270 Nerd Font Mono is 11px a column, not the linear 11.52px. Proportional faces keep their own advances. The drawer's column count is a separate estimate, scaled from a 10×22 cell at 16px, and is only used to size the plate and the line-number gutter.

## Editor

Each tab is an iced text editor buffer. A scratch tab has no path and is stored under the config directory. A file tab reads and writes its path. One scratch tab stays open beside any files. Closing a dirty scratch asks first. Closing the last tab creates a new scratch rather than leaving the drawer empty.

Undo and redo are snapshots of the text and the byte cursor, capped around 100 steps, and they are not written to disk. Indent, outdent, a remembered newline, and a smart backspace are each one step. Find moves the caret and does not push history.

Tab inserts the configured unit at the caret, or indents each selected line. Shift+Tab removes one unit. Backspace on a line made only of that unit deletes back to the previous stop. Enter inserts a newline, and when remember-indent is on it also copies the current line's leading run of whole units. These keys are handled by the focused editor, not the window, so the find field keeps its own Enter.

Find is case-insensitive, wraps, and counts matches. An empty query does not move. Folding compares one source character with one query character.

Word wrap uses cosmic-text's word wrap. Line numbers count buffer lines and follow the layout the editor already drew, so a resize does not shape the file again. A wrapped continuation has no number. The caret line is one highlight behind the gutter and the text, and it does not cover the scrollbar.

Syntax is a fixed list: plain text, Rust, Markdown, Python, TOML, JSON, JavaScript, TypeScript, HTML, CSS, C, C++, Go, shell, Lua, and YAML. Opening a file picks the language from the extension. The menu can override it.

## Layout

The anchor is top, bottom, left, or right. Top and bottom share one saved pixel size. Left and right share the other. Changing edge restores that pair, so a wide short drawer does not become a tall side drawer.

The plate follows the pointer while a grip is held, including after the cursor leaves the plate. During that grab the input region is the whole output, so the resize does not die at the edge. A double-click on a side grip of a top or bottom drawer toggles a full-width span and remembers the width to restore. That click does not hide the drawer.

The tab row sits on the screen-edge side: below the editor for a bottom drawer, above it for the others. Find sits between the editor and the tab row. The row is a few pixels tall and scrolls horizontally. The vertical wheel scrolls it sideways. Each tab has its own close button. There is no overflow chevron.

## Settings

The settings window lists theme, font, size, hinting, word wrap, tab stop, remember indent, opacity, blur, edge, automatic close, display, and syntax. Follow-cursor and a pinned display are mutually exclusive. The pin defaults to the primary display and falls back to it when the saved display is gone. A different resolution reclamps the drawer to that output without forgetting a larger saved size. Themes are the iced set, including the light and dark variants of Solarized, Gruvbox, Catppuccin, Tokyo Night, and Kanagawa. The default is Dracula. Light themes highlight with Inspired GitHub. Dark themes highlight with Base16 Ocean.

Automatic close is on by default. Off ignores a click outside and an unfocus, and Escape still hides the drawer while it is focused. The global hotkey still toggles it. Focus is not kept after a click outside.

## Storage

`$XDG_CONFIG_HOME/subtext`, or `~/.config/subtext`:

- `state.json` holds the anchor, both saved sizes, the active tab, cursor and scroll, the display choice, and the settings.
- `scratch/` holds the text of scratch tabs.
- File tabs store metadata only. The bytes live at the file path.

State is written when the drawer hides, when a setting changes, and when a buffer is saved. Restoring a tab puts the caret back and scrolls the editor to that line.

## Modules

```
src/
  main.rs            daemon or one socket command
  app.rs             layer surface, slide, focus, persistence
  ipc.rs             Unix socket
  state.rs           state.json
  message.rs         iced messages and layer-shell constructors
  geom.rs            edges, cells, slide position
  look.rs            font, points, theme
  blur.rs            KWin blur region
  slide.rs           Wayland connection for the blur hook
  fade.rs            plate opacity for a recorded layer
  glibc_compat.rs    acosf and atan2f symbol wrap
  editor/            tabs, find, indent, wrap, syntax
  ui/                plate, tabs, settings, grips, scrollbar
```

`vendor/` is the patched crates the app will not build without:

- `layershellev` installs the blur hook before the first commit of a layer surface.
- `softbuffer` uses Argb8888 so a transparent margin stays transparent.
- `iced_tiny_skia` clips glyphs to the editor, redraws when hinting changes, and can turn hinting off.
- `cosmic-text` snaps monospace advances to the hinted cell.

`build.rs` adds `--wrap=acosf` and `--wrap=atan2f` on x86_64. `glibc_compat.rs` tail-calls the glibc 2.2.5 versions. The wrappers are not named `acosf` or `atan2f`, or the loader binds the wrap back onto this binary.

# Subtext

Subtext is a resident notes drawer for Wayland. One process stays mapped for the life of the session. A hotkey slides a text plate in from a screen edge, and the same hotkey, Escape, or a click outside slides it away. It is for scratch notes and short code, not a project editor.

The stack is Rust 2024, iced 0.14 with the tiny-skia renderer, and iced_layershell. Text is shaped by cosmic-text and highlighted with the syntect highlighter iced already ships. State is JSON through serde. File dialogs use rfd. There is no async runtime: the hotkey client is a short-lived process that writes one line to a Unix socket.

## Process

`subtext` with no arguments is the daemon. It binds `$XDG_RUNTIME_DIR/subtext.sock` and refuses to start when that socket is already taken. `subtext toggle`, `show`, and `hide` write that word to the socket and exit. If nothing is listening, the client starts the daemon and sends the command again.

The daemon opens the layer surface immediately and leaves it parked off the anchor edge, so the first toggle is a slide rather than a first map.

## Surfaces

The drawer is one layer-shell surface anchored to all four edges with a requested size of 0×0, so it covers the output. The plate is a widget inside that surface. Its input region is the on-screen part of the plate. A parked drawer adds nothing to the region, which makes the rest of the output click-through. An empty region is click-through. A null region would be the whole output.

While the drawer is open its layer is `Top`, the same band a quake-style terminal uses. Overlay clients such as Spectacle then draw above it, and the drawer still covers ordinary windows. Settings is its own overlay surface, so it stays above the drawer. Both drop to `Bottom` while a file dialog is open, because an ordinary xdg dialog cannot cover `Top` or `Overlay`. Keyboard interactivity is exclusive while the drawer is open, on-demand while Settings is open, and none while a dialog is open or the drawer is parked.

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

Compact is off by default. On, the editor padding drops from 16px to 8px and the line-number gutter shrinks to the digits the active tab needs, with a floor of two. Off, the gutter stays four digits wide until the line count needs more. Switching tabs or crossing a digit boundary updates the column count. The window size stays put unless the new gutter no longer fits; the text area changes inside it.

## Settings

The settings window lists theme, font, size, hinting, word wrap, compact layout, tab stop, remember indent, opacity, blur, edge, automatic close, display, and syntax. Follow-cursor and a pinned display are mutually exclusive. The pin defaults to the primary display and falls back to it when the saved display is gone. A different resolution reclamps the drawer to that output without forgetting a larger saved size. Themes are the iced set, including the light and dark variants of Solarized, Gruvbox, Catppuccin, Tokyo Night, and Kanagawa. The default is Dracula. Light themes highlight with Inspired GitHub. Dark themes highlight with Base16 Ocean.

Automatic close is on by default. Off ignores a click outside and an unfocus, and Escape still hides the drawer while it is focused. The global hotkey still toggles it. Focus is not kept after a click outside.

## Storage

`$XDG_CONFIG_HOME/subtext`, or `~/.config/subtext`:

- `state.json` holds the anchor, both saved sizes, the active tab, cursor and scroll, the display choice, and the settings.
- `scratch/` holds the text of scratch tabs. A scratch is written when its text has changed and state is saved, so a restart can restore it.
- File tabs store metadata only. Their bytes are written on Save, Save As, when that tab is closed, and when the drawer hides. Changing a setting or switching tabs does not rewrite them.

State is written when the drawer hides, when a setting changes, and when a buffer is saved. Each write goes to a temporary file and is renamed into place. Restoring a tab that cannot be read leaves the original file untouched. A failed Save As leaves the scratch file in place. Restoring a tab puts the caret back and scrolls the editor to that line.

### Reconciling a file that changed while it was open

A scratch directory on a network share, or a file edited in another program, can change while a buffer is still loaded. Subtext does not poll. `App::show` rereads every tab. `Tab::flush` rereads again immediately before an automatic or explicit write. A settings change still does not rewrite clean files. It only reconciles a buffer that is about to be written.

The baseline is `Tab::synced`: the editor text last loaded or saved. It is not stored in `state.json`. A restart loads the file and that load becomes the baseline, so unsaved edits exist only inside a running process. Comparisons use the editor's own text, after `Content::with_text`, so a newline the editor keeps does not look like a disk change.

`Tab::reconcile_disk` reads the store path (the real file, or the scratch file) and then:

- No path, or an unreadable tab: leave the buffer. A read error other than `NotFound` is printed and returns `Failed`. It does not set `unreadable`, or one transient error would lock the file forever. `Failed` skips the write.
- `NotFound` on a path that has never been read or written is a new scratch. The buffer is kept and a later save may create the file.
- `NotFound` after the file has been seen is a deletion. The buffer is not wiped. With no unsaved text this is `Gone`: automatic saves do not recreate the file, and a scratch tab may still close. With unsaved text this is `Missing`: automatic saves refuse, the tab stays open, and Ctrl+S creates the file only if it is still absent.
- Disk and buffer match: the buffer is clean and in sync. Any conflict flag is cleared.
- The buffer still equals the baseline: this side did not edit. Adopt the disk text, clear undo and redo, and clear the dirty flag. Undo must not be able to restore the stale text and save it over the other copy. An empty file is adopted too. A failed or partial read never reaches this path.
- The disk still equals the baseline: only this buffer changed. Keep it.
- Otherwise both sides diverged from the baseline. `editor::merge::merge` splits on `\n`, keeping the newline on the line, and runs a Myers diff from the baseline to each side. Hunks that do not cover the same baseline lines are combined. An insert sits at the point in front of a baseline line: two different inserts at the same point conflict, and an insert does not conflict with a replacement that ends at that point. Identical edits are kept once. The trace stores one diagonal row per step, so its memory follows the square of the edit distance. If that distance passes 1000, or any side is over 1 MiB or 20,000 lines, the merge stops and installs a short conflict note instead of copying the files into the marker. Undo returns to this computer's text.

A clean merge installs the combined text and records one undo step back to this computer's text. `synced` stays the disk text that was merged against, not the combined buffer. Setting it to the combination would make the next remote edit look like the only change and drop the unwritten local half. The buffer stays dirty when the combination differs from the disk, so the next hide writes it and both sides meet.

A conflict installs markers and sets `conflicted` and `saw_conflict`:

```
<<<<<<< subtext (this computer)
=======
>>>>>>> subtext (other copy)
```

The marker prefix is `editor::merge::OURS_MARK`. `has_conflict_marker` treats a conflict as still open while any line starts with at least four `<` or `>` characters, or is only `=` characters and at least four long. Notes are not scanned for that unless `saw_conflict` is already set, so a divider typed in an ordinary note does not block saves. `saw_conflict` is what makes undo and redo arm the flag again. Automatic flush (`hide`, scratch persist, closing a file tab) returns an error while `conflicted` or `saw_conflict` is set, leaves the file untouched, and leaves the tab open. Closing a scratch deletes its file only when `reconcile_disk` returns `Unchanged` or `Gone`. A reload, merge, conflict, failed read, or deletion with unsaved text leaves the file and the tab in place.

Editing every marker line out clears both flags, so the next hide can save the resolution. Removing or shortening only one of them does not. Undo back to this computer's pre-conflict text removes the markers but leaves `saw_conflict` set, so that hide still will not replace the other copy. Ctrl+S and Save As are explicit: they write the buffer even when it is marked, then clear the flags and set the baseline to the written text. Save As still reconciles first, so a clean merge is what gets written to the new path.

`flush` tries the write up to four times. Each attempt locks the destination (`name.subtext-lock`, created exclusively). The lock file records a version, this machine's id, the pid, and that process's `/proc` start time. On this machine a lock is stolen only when that process is gone, or the pid was reused. A suspended or slow writer keeps the lock however old the file is. A lock from another machine, or one that cannot be identified, is stolen only after ten minutes. The file is replaced only when the bytes still equal `synced`. A missing file is never that match, including when `synced` is empty; a path that has not been created yet uses `atomic_write_if_absent`. A unique temp name (`name.subtext-<pid>.<nanos>.<seq>`) keeps two processes from sharing one temporary file. If the bytes changed, the lock is released and the attempt reconciles against the new text. That closes the window where both sides read the same baseline and the later rename drops the earlier save. The lock is also taken for an unconditional `atomic_write`, including `state.json`.

Share `scratch/` only. `state.json` stays in each machine's config directory. A symlink of the whole directory would make the two machines rewrite one layout file.

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
  editor/            tabs, find, indent, wrap, syntax, merge
  ui/                plate, tabs, settings, grips, scrollbar
```

`vendor/` is the patched crates the app will not build without:

- `layershellev` installs the blur hook before the first commit of a layer surface.
- `softbuffer` uses Argb8888 so a transparent margin stays transparent.
- `iced_tiny_skia` clips glyphs to the editor, redraws when hinting changes, and can turn hinting off.
- `cosmic-text` snaps monospace advances to the hinted cell.

`build.rs` adds `--wrap=acosf` and `--wrap=atan2f` on x86_64. `glibc_compat.rs` tail-calls the glibc 2.2.5 versions. The wrappers are not named `acosf` or `atan2f`, or the loader binds the wrap back onto this binary.

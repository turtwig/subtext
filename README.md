# Subtext

A resident Wayland drawer for notes and short bits of code. It stays running and hidden until a hotkey shows it, then slides in from the screen edge it is anchored to. The default edge is the bottom, so it does not cover a drop-down terminal.

```
subtext                 # start hidden and keep running
subtext toggle          # show or hide
subtext show
subtext hide
```

The daemon listens on `$XDG_RUNTIME_DIR/subtext.sock`. A second `subtext` exits if one is already running. `toggle`, `show`, and `hide` send that command to the daemon, and start it first when nothing is listening.

Hyprland:

```
exec-once = subtext
bind = SUPER, GRAVE, exec, subtext toggle
```

Sway:

```
exec subtext
bindsym $mod+grave exec subtext toggle
```

On KDE, bind `subtext toggle` to a global shortcut. Background blur uses the KWin blur protocol and does nothing on other compositors.

## Editing

Escape closes Settings, cancels a scratch-close prompt, closes the find bar, and then hides the drawer. When automatic close is on, a click outside the drawer also hides it. Closing a scratch tab that still has text asks first.

Ctrl+F finds in the current note. The search is case-insensitive. Enter and F3 move to the next match. Shift+Enter and Shift+F3 move to the previous one. The query is not saved.

Ctrl+S saves, Ctrl+O opens, Ctrl+N adds a scratch tab, and Ctrl+W closes the current tab. Ctrl+Z undoes. Ctrl+Shift+Z and Ctrl+Y redo. There is always at least one scratch tab, including while files are open. Closing the last tab leaves a fresh scratch. Hiding the drawer writes every open buffer.

Tab inserts the indent chosen in Settings: a tab character, two spaces, or four spaces. Shift+Tab removes one. On a line that is only that indent, Backspace removes one stop, or the leftover spaces past the last stop. When Remember indent is on, Enter repeats the indent of the current line.

## Layout

Drag a free edge and that edge follows the pointer, including after the cursor leaves the drawer. A top or bottom drawer stays centered, so a side drag grows the width on both sides. A left or right drawer does the same for its height. The anchored edge stays put.

Columns and rows on the tab bar are how many character cells fit. They are not the drag step. The default is 120 columns, and the row count starts near 40% of a 1080p screen. Top and bottom share one saved size. Left and right share another. Double-click either side edge of a top or bottom drawer to stretch it across the output, and double-click again to return to the previous width.

The drawer slides in from the anchored edge. The surface itself stays mapped and fullscreen. Only the plate moves, and only the plate accepts clicks.

## Settings

Settings is its own window, opened from the tab row, and the editor stays visible behind it. The controls are theme, installed monospace font, text size, glyph hinting, word wrap, tab characters, remember indent, background opacity, KWin blur, anchor edge, automatic close, and syntax. Changes apply immediately and are stored with the rest of the state.

Text size is in points, matching a terminal at 96 DPI. The control next to the number switches between points and pixels without jumping the glyphs. System is the generic monospace face. Other choices are monospace families installed on the machine. Hinting is on by default. For a monospace face, each column is the hinted pixel width of that face at the current size, so a 16pt terminal cell and a Subtext cell land on the same pixels.

Opacity fades the drawer plate. Blur, on KWin, frosts whatever is behind that plate. It has no separate strength. Lines on the tab row toggles line numbers. The numbers are smaller and dimmer than the buffer, follow the caret line, and skip rows that exist only because a line wrapped. Syntax colors follow the UI theme: light themes use Inspired GitHub, and dark themes use Base16 Ocean.

## Storage

Open files, scratch text, the cursor, scroll position, anchor edge, sizes, and settings are stored under `$XDG_CONFIG_HOME/subtext` (or `~/.config/subtext`).

- `state.json` is the metadata.
- `scratch/` holds scratch tabs (`scratch_1.txt`, and so on).
- A real file is written at its own path.

## Build

```bash
cargo build --release
mkdir -p ~/.local/bin
cp target/release/subtext ~/.local/bin/subtext
```

`~/.local/bin` has to be on `PATH`. Start it from a terminal inside the Wayland session. If a drawer is already resident, `pkill subtext` before starting the new one.

On x86_64, the link wraps `acosf` and `atan2f` so the binary uses the glibc 2.2.5 versions of those two symbols. A host with glibc 2.41 can run that release.

## License

MIT. Use it for whatever you like. The notice is in `LICENSE`.

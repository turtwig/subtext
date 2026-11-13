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

On KDE, bind `subtext toggle` to a global shortcut. Background blur uses `ext_background_effect_v1` when the compositor advertises it, and the older KWin blur protocol otherwise. It does nothing when neither is available.

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

Settings is its own window, opened from the tab row, and the editor stays visible behind it. The controls are theme, editor font, UI font, text size, glyph hinting, word wrap, tab characters, remember indent, background opacity, KWin blur, anchor edge, border thickness, automatic close, and syntax. Changes apply immediately and are stored with the rest of the state.

Text size is in points, matching a terminal at 96 DPI. The control next to the number switches between points and pixels without jumping the glyphs. System is the generic monospace face. Other choices are monospace families installed on the machine. The UI font defaults to the editor face. A separate toggle makes the tab row and find bar use the editor's pixel size; otherwise that chrome stays at 14px. The settings window stays at 14px either way, so its layout does not move. Hinting is on by default. For a monospace face, each column is the hinted pixel width of that face at the current size, so a 16pt terminal cell and a Subtext cell land on the same pixels.

The resize edges are a solid bar, in pixels, from 0 to 24. The bar runs the whole edge, and the text sits against it. Zero draws nothing. The drag target stays a few pixels wide and overlaps the text when the bar is thinner than that.

Opacity fades the drawer plate. Blur, on KWin, frosts whatever is behind that plate. It has no separate strength. Lines on the tab row toggles line numbers. The numbers are smaller and dimmer than the buffer, follow the caret line, and skip rows that exist only because a line wrapped. Syntax colors follow the UI theme: light themes use Inspired GitHub, and dark themes use Base16 Ocean.

## Storage

Open files, scratch text, the cursor, scroll position, anchor edge, sizes, and settings are stored under `$XDG_CONFIG_HOME/subtext` (or `~/.config/subtext`).

- `state.json` is the metadata.
- `scratch/` holds scratch tabs (`scratch_1.txt`, and so on).
- A real file is written at its own path.

Share notes by symlinking only `scratch/` at a shared folder. Leave `state.json` on each machine, so window size, fonts, and the open tabs stay local. Symlinking the whole config directory would share that file too, and the two machines would overwrite each other's layout. Subtext does not watch the files while the drawer is open. It rereads them when the drawer is shown, and again immediately before it writes.

Each open buffer remembers the text it last loaded or saved. That remembered text is the baseline. The next read compares three copies: the baseline, the text in the buffer, and the file on disk.

- Only the file changed, and this buffer was not edited. The buffer becomes the file, including a full rewrite. Undo is cleared, so it cannot put the old text back and save it.
- Only this buffer changed. The buffer is kept and written as usual.
- Both changed, on different lines. The two edits are combined. Undo once to return to the text from this computer. The other copy stays the baseline, so a later edit on the other computer is merged again instead of discarding this half.
- Both changed on the same lines, or both appended different text at the same place. The buffer is marked and nothing is written over the file:

```
<<<<<<< subtext (this computer)
this computer's lines
=======
the other copy's lines
>>>>>>> subtext (other copy)
```

Delete every marker line and keep the text you want. A marker line is one that still starts with `<<<<` or `>>>>`, or a line made only of `=` signs. Shortening or deleting just the first line does not resolve it. The next hide then saves. Ctrl+S writes the buffer as it stands, markers included, and that becomes the new baseline. Hiding, closing the tab, or any other automatic save will not overwrite a conflict, and will not delete a scratch whose file changed on disk. Undoing back to this computer's text does not count as resolving it. A failed reread leaves both the buffer and the file alone.

If the file is deleted while this buffer has unsaved text, Subtext keeps the buffer and does not recreate the file. Ctrl+S writes it again. A scratch that has never been saved is still created on the next save.

A save locks the file and replaces it only when it still contains the bytes just read. A missing file is not treated as empty, so a deleted note is not recreated by that check. If the other computer saved first, this one reads that copy and merges instead of writing over it. Two saves that pass the lock do not share a temporary file. The lock records this machine and the process. A live process is not interrupted. A lock from another machine is only taken after ten minutes.

The same rules apply when a file is edited in another program while it is open here. Both computers need this version. A drawer that is already open picks the other copy up the next time it opens, and before it saves.

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

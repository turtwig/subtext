//! Messages for the drawer. Layer-shell actions are generated onto this enum
//! so the UI can name them without depending on the daemon.

use crate::editor::{Language, TabStop};
use crate::geom::Edge;
use crate::ipc::Command;
use crate::look::{FontChoice, ThemeChoice};
use iced::widget::text_editor;
use iced::{Point, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use iced_layershell::to_layer_message;
use std::path::PathBuf;

/// Opens a layer surface. The generated helper is private to this module.
pub fn open_layer(settings: NewLayerShellSettings) -> (window::Id, iced::Task<Message>) {
    Message::layershell_open(settings)
}

/// Client size of the settings window.
pub const SETTINGS_SIZE: (u32, u32) = (460, 660);

/// Opens the settings window on the overlay layer, centered on the output.
///
/// An xdg window cannot stack above the drawer: normal windows sit under
/// both `Top` and `Overlay`. `Bottom` hides the drawer behind the desktop.
pub fn open_settings() -> (window::Id, iced::Task<Message>) {
    Message::layershell_open(NewLayerShellSettings {
        size: Some(SETTINGS_SIZE),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        exclusive_zone: Some(0),
        margin: Some((0, 0, 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("subtext-settings".into()),
    })
}

/// Which character axis a resize grip changes, and which drag direction grows it.
#[derive(Debug, Clone, Copy)]
pub struct Grip {
    pub columns: i32,
    pub rows: i32,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Ipc(Command),
    Edit(text_editor::Action),
    Select(usize),
    Close(usize),
    CloseActive,
    ConfirmClose,
    CancelClose,
    NewTab,
    Open,
    Opened(Option<PathBuf>),
    Save,
    SaveAs,
    SavedAs(Option<PathBuf>),
    Language(Language),
    Theme(ThemeChoice),
    Font(FontChoice),
    FontSmaller,
    FontLarger,
    SetTextUnit(crate::look::TextUnit),
    SetEdge(Edge),
    ToggleLineNumbers,
    ToggleWrap,
    /// Tight padding and gutter around the glyphs.
    ToggleCompact,
    TabStop(TabStop),
    /// Enter repeats the current line's indent when this is on.
    ToggleRememberIndent,
    ToggleHinting,
    /// When on, a click outside the drawer hides it.
    ToggleClickAway,
    /// Insert the configured tab unit at the caret, or before each selected line.
    Indent,
    /// Break the line at the caret. Remember-indent may repeat the prefix.
    BreakLine,
    /// Backspace. An indent-only line drops one tab stop.
    Backspace,
    /// Remove one tab unit from the caret line, or from each selected line.
    Unindent,
    Undo,
    Redo,
    /// Open the find bar, or focus it when it is already open.
    Find,
    FindQuery(String),
    /// Enter was pressed in the find field. [`Message::FindEnter`] carries Shift.
    FindSubmit,
    FindEnter {
        shift: bool,
    },
    FindNext,
    FindPrevious,
    FindClose,
    ToggleBlur,
    /// Drawer and settings plate alpha, from 0.2 to 1.
    Opacity(f32),
    ToggleSettings,
    /// Double-click a side grip. Top and bottom mode toggles full output width.
    ToggleWidth,
    /// Vertical wheel over the tab strip. `delta` is horizontal pixels.
    ScrollTabs {
        window: window::Id,
        delta: f32,
    },
    Grip(Grip),
    /// Press on the editor scrollbar. The cursor position decides where it grabs.
    ScrollGrab,
    Cursor(window::Id, Point),
    DragEnd,
    /// Escape, or a click away from the drawer.
    Dismiss,
    /// A window mapped. The size is the output for the drawer. Only the
    /// drawer should take editor focus.
    SurfaceOpened(window::Id, f32, f32),
    /// Left press on a surface. A press outside the drawer dismisses it.
    PointerPressed(window::Id),
    Hide,
    Show,
    Toggle,
    Unfocused(window::Id),
    Resized(window::Id, f32, f32),
    Closed(window::Id),
    /// One frame of the in-window slide.
    SlideFrame,
}

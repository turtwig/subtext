//! Resident layer-shell drawer. The fullscreen surface stays mapped. `show`
//! slides the drawer in from its edge; `hide` slides it back off and leaves
//! the surface clear, so the compositor never runs its own open or close
//! animation over the panel.

use crate::editor::{Tab, TabStop};
use crate::geom::{self, Cell, Edge, FALLBACK_OUTPUT, default_rows, pixel_size};
use crate::ipc::Command;
use crate::look::{self, FontChoice, TextUnit, ThemeChoice, UiFont};
use crate::message::Message;
use crate::state::{self, Paths, Snapshot};
use crate::ui::Drawer;
use iced::{Element, Event, Font, Point, Subscription, Task, Theme, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const UNFOCUS_GRACE: Duration = Duration::from_millis(300);
/// KWin's generic open animation. The first map stays blank until this passes,
/// then the drawer slides itself in.
const INTRO_HOLD: Duration = Duration::from_millis(300);

#[derive(Clone, Copy)]
struct Motion {
    from: f32,
    to: f32,
    started: Instant,
}

#[derive(Clone, Copy)]
enum Gesture {
    Armed {
        columns_sign: i32,
        rows_sign: i32,
    },
    Dragging {
        columns_sign: i32,
        rows_sign: i32,
        /// Press position in drawer-local pixels. Pointer events are output-relative.
        origin: Point,
        start_width: u32,
        start_height: u32,
    },
    /// `grab` is the pointer offset inside the scrollbar thumb.
    Scroll {
        grab: f32,
    },
}

pub struct App {
    paths: Paths,
    tabs: Vec<Tab>,
    active: usize,
    edge: Edge,
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
    width_restore: u32,
    /// Top or bottom drawer is as wide as the output. The surface itself
    /// always covers the output; this only changes the opaque rectangle.
    span_width: bool,
    /// Top and bottom size. Left and right keep `side_width` and `side_height`.
    across_width: u32,
    across_height: u32,
    across_restore: u32,
    across_span: bool,
    side_width: u32,
    side_height: u32,
    opacity: f32,
    blur: bool,
    /// A resize grip is held. The input region is the whole output until release.
    capturing: bool,
    rows_custom: bool,
    output_width: Option<u32>,
    output_height: Option<u32>,
    pending_close: Option<usize>,
    /// Number shown in Settings. `text_unit` says whether it is points or pixels.
    text_size: f32,
    text_unit: TextUnit,
    font: FontChoice,
    ui_font: UiFont,
    ui_match_size: bool,
    /// Painted resize edge, in pixels. The drag target is never thinner than [`geom::GRAB`].
    border: u32,
    theme: ThemeChoice,
    line_numbers: bool,
    word_wrap: bool,
    /// Tight padding and gutter around the glyphs.
    compact: bool,
    tab_stop: TabStop,
    remember_indent: bool,
    hinting: bool,
    /// Clicking outside the drawer hides it.
    click_away: bool,
    /// Slide out on whichever display the pointer is on.
    follow_cursor: bool,
    /// Pinned output. Empty means the primary display.
    output_name: String,
    /// Connected displays. Empty until a Wayland round-trip succeeds.
    screens: Vec<crate::output::Screen>,
    /// Output the current drawer surface was created on.
    bound_output: Option<String>,
    /// The surface is closing so it can be opened on `bound_output`.
    reopen_drawer: bool,
    /// 0 is fully past the anchor edge. 1 is the resting rectangle.
    slide: f32,
    motion: Option<Motion>,
    /// When the fullscreen surface was mapped. A show before [`INTRO_HOLD`]
    /// waits, so KWin's scale animation only ever sees an empty frame.
    mapped_at: Option<Instant>,
    hold_until: Option<Instant>,
    /// `show` arrived before the surface existed.
    open_when_mapped: bool,
    find_open: bool,
    find_query: String,
    /// Enter in the find field set this. The matching key event arrives next.
    find_submit: bool,
    settings_window: Option<window::Id>,
    surface: Option<window::Id>,
    gesture: Option<Gesture>,
    cursor: Option<Point>,
    dialog_open: bool,
    suppress_unfocus_until: Instant,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::load();
        app.refresh_screens();
        app.bound_output = app.resolved_output();
        // Map once, blank, before the user asks. KWin's open animation then
        // runs against an empty frame at startup instead of on every toggle.
        let open = app.open_surface();
        (app, open)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.note_editor_scroll();
        let gutter = self.gutter();
        let task = self.dispatch(message);
        if self.gutter() != gutter {
            self.reclamp();
        }
        task
    }

    fn dispatch(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ipc(Command::Toggle) | Message::Toggle => self.toggle(),
            Message::Ipc(Command::Show) | Message::Show => self.show(),
            Message::Ipc(Command::Hide) | Message::Hide => self.hide(),
            Message::Dismiss => {
                if self.settings_window.is_some() {
                    self.close_settings()
                } else if self.pending_close.take().is_some() {
                    Task::none()
                } else if self.find_open {
                    self.close_find()
                } else if self.dialog_open || self.gesture.is_some() {
                    Task::none()
                } else {
                    self.hide()
                }
            }
            Message::Edit(action) => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.edit(action);
                }
                Task::none()
            }
            Message::Select(index) => self.select(index),
            Message::Close(index) => self.request_close(index),
            Message::CloseActive => self.request_close(self.active),
            Message::ConfirmClose => self.confirm_close(),
            Message::CancelClose => {
                self.pending_close = None;
                self.focus_editor()
            }
            Message::ScrollTabs { window, delta } => {
                if self.surface != Some(window) || !self.over_tab_bar() {
                    Task::none()
                } else {
                    self.scroll_tabs(delta)
                }
            }
            Message::NewTab => self.new_tab(),
            Message::Open => self.begin_dialog(false),
            Message::SaveAs => self.begin_dialog(true),
            Message::Save => self.save(),
            Message::Opened(path) => self.finish_open(path),
            Message::SavedAs(path) => self.finish_save_as(path),
            Message::Language(language) => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.language = language;
                }
                self.persist();
                Task::none()
            }
            Message::Theme(theme) => {
                self.theme = theme;
                self.persist();
                Task::none()
            }
            Message::Font(font) => {
                if font == self.font {
                    return Task::none();
                }
                self.font = font;
                self.persist();
                Task::none()
            }
            Message::FontUi(font) => {
                if font == self.ui_font {
                    return Task::none();
                }
                self.ui_font = font;
                self.persist();
                Task::none()
            }
            Message::FontSmaller => self.set_text_size(self.text_size - 1.0),
            Message::FontLarger => self.set_text_size(self.text_size + 1.0),
            Message::ToggleUiSize => {
                self.ui_match_size = !self.ui_match_size;
                self.sync_cells();
                self.persist();
                self.refrost();
                Task::none()
            }
            Message::BorderSmaller => self.set_border(self.border.saturating_sub(1)),
            Message::BorderLarger => self.set_border(self.border.saturating_add(1)),
            Message::SetTextUnit(unit) => self.set_text_unit(unit),
            Message::SetEdge(edge) => self.set_edge(edge),
            Message::ToggleLineNumbers => self.toggle_line_numbers(),
            Message::ToggleWrap => {
                self.word_wrap = !self.word_wrap;
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.reveal_caret();
                }
                self.persist();
                Task::none()
            }
            Message::ToggleCompact => {
                self.compact = !self.compact;
                self.reclamp();
                self.persist();
                self.refrost();
                Task::none()
            }
            Message::TabStop(tab_stop) => {
                if tab_stop == self.tab_stop {
                    return Task::none();
                }
                self.tab_stop = tab_stop;
                self.persist();
                Task::none()
            }
            Message::ToggleRememberIndent => {
                self.remember_indent = !self.remember_indent;
                self.persist();
                Task::none()
            }
            Message::ToggleHinting => {
                self.hinting = !self.hinting;
                self.persist();
                Task::none()
            }
            Message::ToggleClickAway => {
                self.click_away = !self.click_away;
                self.persist();
                Task::none()
            }
            Message::ToggleFollowCursor => self.toggle_follow(),
            Message::ShowOn(name) => self.show_on(name),
            Message::Indent => self.indent_active(false),
            Message::BreakLine => self.break_line(),
            Message::Backspace => self.backspace(),
            Message::Unindent => self.indent_active(true),
            Message::Undo => self.history(true),
            Message::Redo => self.history(false),
            Message::Find => self.open_find(),
            Message::FindQuery(query) => {
                self.find_query = query;
                self.find_seek()
            }
            Message::FindSubmit => {
                self.find_submit = true;
                Task::none()
            }
            Message::FindEnter { shift } => {
                if !self.find_open || !self.find_submit {
                    self.find_submit = false;
                    return Task::none();
                }
                self.find_submit = false;
                self.find_from_caret(shift)
            }
            Message::FindNext => self.find_from_caret(false),
            Message::FindPrevious => self.find_from_caret(true),
            Message::FindClose => self.close_find(),
            Message::ToggleBlur => {
                self.blur = !self.blur;
                self.follow_blur();
                self.persist();
                Task::none()
            }
            Message::Opacity(opacity) => {
                let opacity = clamp_opacity(opacity);
                if (opacity - self.opacity).abs() < f32::EPSILON {
                    return Task::none();
                }
                self.opacity = opacity;
                self.persist();
                Task::none()
            }
            Message::ToggleSettings => self.toggle_settings(),
            Message::ToggleWidth => {
                self.gesture = None;
                self.capturing = false;
                // The pointer is on the old edge. After a shrink that point
                // is outside the drawer, and the same click would dismiss it.
                self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
                if matches!(self.edge, Edge::Top | Edge::Bottom) {
                    self.toggle_full_width()
                } else {
                    self.hit_region()
                }
            }
            Message::Grip(grip) => {
                self.capturing = true;
                self.gesture = Some(match self.cursor {
                    Some(origin) => self.dragging(grip.columns, grip.rows, origin),
                    None => Gesture::Armed {
                        columns_sign: grip.columns,
                        rows_sign: grip.rows,
                    },
                });
                // The surface is already the output. Widen the hit target so
                // the drag keeps going after the pointer leaves the drawer.
                self.hit_region()
            }
            Message::ScrollGrab => self.begin_scroll(),
            Message::Cursor(id, point) => {
                if self.surface != Some(id) {
                    Task::none()
                } else {
                    self.cursor(point)
                }
            }
            Message::DragEnd => {
                let ended = self.gesture.take();
                let was_capturing = self.capturing;
                self.capturing = false;
                if matches!(ended, Some(Gesture::Dragging { .. })) {
                    self.reclamp();
                    self.note_custom_width();
                    self.persist();
                }
                if was_capturing {
                    self.hit_region()
                } else {
                    Task::none()
                }
            }
            Message::SurfaceOpened(id, width, height) => {
                if self.surface != Some(id) {
                    return Task::none();
                }
                // The first configure is reported as Opened, not Resized.
                // Without that size a drag has no output to measure against.
                self.mapped_at = Some(Instant::now());
                let noted = self.note_output(width, height);
                if self.open_when_mapped {
                    self.open_when_mapped = false;
                    return Task::batch([noted, self.begin_slide(1.0)]);
                }
                if self.wants_open() {
                    Task::batch([noted, self.focus_at_cursor()])
                } else {
                    noted
                }
            }
            Message::PointerPressed(id) => self.press_outside(id),
            Message::Unfocused(id) => {
                if !self.click_away
                    || self.surface != Some(id)
                    || self.settings_window.is_some()
                    || self.dialog_open
                    || self.gesture.is_some()
                    || Instant::now() < self.suppress_unfocus_until
                {
                    Task::none()
                } else {
                    self.hide()
                }
            }
            Message::Resized(id, width, height) => self.resized(id, width, height),
            Message::Closed(id) => {
                if self.settings_window == Some(id) {
                    self.settings_window = None;
                    self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
                    return self.restore_after_settings();
                }
                if self.surface == Some(id) {
                    self.surface = None;
                    self.slide = 0.0;
                    self.motion = None;
                    self.hold_until = None;
                    self.mapped_at = None;
                    if self.reopen_drawer {
                        self.reopen_drawer = false;
                        return self.open_surface();
                    }
                    self.open_when_mapped = false;
                    self.persist_documents();
                    return self.close_settings();
                }
                Task::none()
            }
            Message::SlideFrame => self.slide_frame(),
            _ => Task::none(),
        }
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message> {
        iced_tiny_skia::set_hinting(self.hinting);
        if self.settings_window == Some(id) {
            let language = self
                .tabs
                .get(self.active)
                .map(|tab| tab.language)
                .unwrap_or(crate::editor::Language::Plain);
            return crate::ui::settings::view(crate::ui::settings::Model {
                theme: self.theme,
                font: self.font,
                ui_font: self.ui_font,
                text_size: self.text_size,
                text_unit: self.text_unit,
                ui_match_size: self.ui_match_size,
                border: self.border,
                word_wrap: self.word_wrap,
                compact: self.compact,
                tab_stop: self.tab_stop,
                remember_indent: self.remember_indent,
                hinting: self.hinting,
                click_away: self.click_away,
                opacity: self.opacity,
                blur: self.blur,
                edge: self.edge,
                language,
                follow_cursor: self.follow_cursor,
                screens: self.screens.clone(),
                selected_output: if self.follow_cursor {
                    None
                } else {
                    crate::output::choose(false, &self.output_name, &self.screens, None)
                },
            });
        }
        let confirm_close = self
            .pending_close
            .and_then(|index| self.tabs.get(index).map(|tab| tab.title.as_str()));
        if !self.drawer_painted() {
            return clear_output();
        }
        let Some(output) = self.output_width.zip(self.output_height) else {
            return clear_output();
        };
        let body = crate::ui::drawer(Drawer {
            tabs: &self.tabs,
            active: self.active,
            edge: self.edge,
            columns: self.columns,
            rows: self.rows_on_screen(),
            pad: geom::editor_pad(self.compact),
            gutter: self.gutter(),
            text_size: self.text_pixels(),
            font: self.font.font(),
            ui_font: self.ui_font.resolve(self.font),
            ui_size: self.ui_pixels(),
            border: self.border,
            bar: self.bar_px(),
            find_bar: self.find_height(),
            syntax: self.theme.syntax(),
            line_numbers: self.line_numbers,
            word_wrap: self.word_wrap,
            confirm_close,
            find_open: self.find_open,
            find_query: &self.find_query,
        });
        // The surface is the whole output for its whole life. The drawer is a
        // rectangle on it, shifted along the anchor while it slides.
        crate::ui::capture_frame(
            self.edge,
            output,
            (self.width, self.height),
            self.slide,
            self.plate_opacity(),
            body,
        )
    }

    pub fn theme(&self) -> Theme {
        self.theme.iced()
    }

    pub fn subscription(state: &Self) -> Subscription<Message> {
        let mut parts = vec![
            iced::event::listen_with(on_event),
            crate::ipc::subscription().map(Message::Ipc),
            window::close_events().map(Message::Closed),
        ];
        if state.motion.is_some() || state.hold_until.is_some() {
            parts.push(Subscription::run(slide_frames).map(|_| Message::SlideFrame));
        }
        Subscription::batch(parts)
    }

    pub fn appearance(_state: &Self, theme: &Theme) -> iced::theme::Style {
        let palette = theme.palette();
        let mut background_color = palette.background;
        // The drawer surface covers the output and stays clear outside the
        // drawer. Settings paints its own opaque plate on top of this.
        background_color.a = 0.0;
        iced::theme::Style {
            background_color,
            text_color: palette.text,
        }
    }

    fn load() -> Self {
        let paths = Paths::resolve();
        match state::read(&paths) {
            Ok(Some(snapshot)) => Self::from_snapshot(paths, snapshot),
            Ok(None) => Self::fresh(paths),
            Err(error) => {
                eprintln!("subtext: could not read state ({error}); starting fresh");
                Self::fresh(paths)
            }
        }
    }

    fn fresh(paths: Paths) -> Self {
        let mut app = Self::blank(paths);
        app.rows = default_rows(
            app.edge,
            FALLBACK_OUTPUT.1,
            app.cell(),
            geom::Frame::at(geom::editor_pad(false)),
        );
        app.ensure_pixels();
        app.tabs.push(Tab::new_scratch(&app.paths.scratch, 1));
        app
    }

    fn blank(paths: Paths) -> Self {
        Self {
            paths,
            tabs: Vec::new(),
            active: 0,
            edge: Edge::Bottom,
            columns: geom::DEFAULT_COLS,
            rows: geom::MIN_ROWS,
            width: 0,
            height: 0,
            width_restore: 0,
            span_width: false,
            across_width: 0,
            across_height: 0,
            across_restore: 0,
            across_span: false,
            side_width: 0,
            side_height: 0,
            opacity: 1.0,
            blur: false,
            capturing: false,
            rows_custom: false,
            output_width: None,
            output_height: None,
            pending_close: None,
            text_size: geom::TEXT_SIZE,
            text_unit: TextUnit::Pt,
            font: FontChoice::SYSTEM,
            ui_font: UiFont::MatchEditor,
            ui_match_size: false,
            border: 1,
            theme: ThemeChoice::Dracula,
            line_numbers: true,
            word_wrap: false,
            compact: false,
            tab_stop: TabStop::Spaces4,
            remember_indent: false,
            hinting: true,
            click_away: true,
            follow_cursor: false,
            output_name: String::new(),
            screens: Vec::new(),
            bound_output: None,
            reopen_drawer: false,
            slide: 0.0,
            motion: None,
            mapped_at: None,
            hold_until: None,
            open_when_mapped: false,
            find_open: false,
            find_query: String::new(),
            find_submit: false,
            settings_window: None,
            surface: None,
            gesture: None,
            cursor: None,
            dialog_open: false,
            suppress_unfocus_until: Instant::now(),
        }
    }

    fn from_snapshot(paths: Paths, snapshot: Snapshot) -> Self {
        let mut app = Self::blank(paths);
        app.edge = snapshot.anchor;
        app.columns = snapshot.columns;
        app.rows = snapshot.rows;
        app.rows_custom = true;
        app.text_unit = snapshot.text_unit;
        app.text_size = look::clamp_text_size(snapshot.text_size, snapshot.text_unit);
        app.font = snapshot.font;
        app.ui_font = snapshot.ui_font;
        app.ui_match_size = snapshot.ui_match_size;
        app.border = geom::clamp_border(snapshot.border);
        app.theme = snapshot.theme;
        app.line_numbers = snapshot.line_numbers;
        app.word_wrap = snapshot.word_wrap;
        app.compact = snapshot.compact;
        app.hinting = snapshot.hinting;
        app.click_away = snapshot.click_away;
        app.follow_cursor = snapshot.follow_cursor;
        app.output_name = snapshot.output_name;
        app.width = snapshot.width;
        app.height = snapshot.height;
        app.width_restore = snapshot.width_restore;
        app.span_width = snapshot.span_width && matches!(snapshot.anchor, Edge::Top | Edge::Bottom);
        app.across_width = snapshot.across_width;
        app.across_height = snapshot.across_height;
        app.across_restore = snapshot.across_width_restore;
        app.across_span = snapshot.across_span;
        app.side_width = snapshot.side_width;
        app.side_height = snapshot.side_height;
        app.opacity = clamp_opacity(snapshot.opacity);
        app.blur = snapshot.blur;
        app.tab_stop = snapshot.tab_stop;
        app.remember_indent = snapshot.remember_indent;
        app.tabs = snapshot.tabs.into_iter().map(Tab::from_record).collect();
        app.ensure_scratch();
        app.active = snapshot.active.min(app.tabs.len() - 1);
        app.ensure_pixels();
        app.remember_active();
        app
    }

    fn toggle(&mut self) -> Task<Message> {
        if self.wants_open() {
            self.hide()
        } else {
            self.show()
        }
    }

    fn show(&mut self) -> Task<Message> {
        self.reconcile_tabs();
        self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
        self.recall_cursor();
        self.refresh_screens();
        let target = self.resolved_output();
        if self.bound_output != target {
            self.open_when_mapped = true;
            return self.retarget(target);
        }
        if self.surface.is_none() {
            self.open_when_mapped = true;
            return self.open_surface();
        }
        if self.resting() {
            return self.focus_editor();
        }
        if self.hold_until.is_some()
            || self.open_when_mapped
            || self.motion.as_ref().is_some_and(|motion| motion.to >= 1.0)
        {
            return Task::none();
        }
        self.begin_slide(1.0)
    }

    fn open_surface(&mut self) -> Task<Message> {
        if self.surface.is_some() {
            return Task::none();
        }
        let (id, task) = crate::message::open_layer(self.layer_settings());
        self.surface = Some(id);
        task
    }

    /// The user wants the drawer on screen, including while it is still arriving.
    fn wants_open(&self) -> bool {
        if self.open_when_mapped || self.hold_until.is_some() {
            return true;
        }
        match &self.motion {
            Some(motion) => motion.to >= 1.0,
            None => self.slide >= 1.0,
        }
    }

    /// Resting fully open. Blur is only installed in this state.
    fn resting(&self) -> bool {
        self.slide >= 1.0 && self.motion.is_none() && self.hold_until.is_none()
    }

    /// Draw the drawer. A parked surface stays fully clear.
    fn drawer_painted(&self) -> bool {
        self.motion.is_some() || self.slide > 0.0
    }

    fn open_find(&mut self) -> Task<Message> {
        if self.settings_window.is_some() || self.dialog_open || !self.wants_open() {
            return Task::none();
        }
        let already = self.find_open;
        self.find_open = true;
        if !already {
            let _ = self.find_seek();
        }
        self.focus_find()
    }

    fn close_find(&mut self) -> Task<Message> {
        self.find_open = false;
        self.find_submit = false;
        self.focus_editor()
    }

    fn focus_find(&self) -> Task<Message> {
        let focus = iced::advanced::widget::operate(
            iced::advanced::widget::operation::focusable::focus(crate::ui::FIND_ID.clone()),
        );
        let select = iced::advanced::widget::operate(
            iced::advanced::widget::operation::text_input::select_all(crate::ui::FIND_ID.clone()),
        );
        focus.chain(select)
    }

    /// Move to the match at the caret, or the next one when the caret is already
    /// on a match. Typing in the find field uses this so a longer query stays
    /// on the same hit when it still matches.
    fn find_seek(&mut self) -> Task<Message> {
        self.jump_find(true, false)
    }

    fn find_from_caret(&mut self, backward: bool) -> Task<Message> {
        self.jump_find(false, backward)
    }

    fn jump_find(&mut self, seek: bool, backward: bool) -> Task<Message> {
        if !self.find_open || self.find_query.is_empty() {
            return Task::none();
        }
        let query = self.find_query.clone();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return Task::none();
        };
        let cursor = tab.content.cursor();
        let caret = crate::editor::find::Span {
            line: cursor.position.line,
            column: cursor.position.column,
        };
        let anchor = cursor.selection.map(|position| crate::editor::find::Span {
            line: position.line,
            column: position.column,
        });
        let text = tab.text();
        let place = if seek {
            let origin = match anchor {
                Some(anchor) => crate::editor::find::earlier(caret, anchor),
                None => caret,
            };
            crate::editor::find::seek(&text, &query, origin)
        } else {
            crate::editor::find::step(&text, &query, caret, anchor, backward)
        };
        let Some(place) = place else {
            return Task::none();
        };
        tab.select_range(
            place.start.line,
            place.start.column,
            place.end.line,
            place.end.column,
        );
        Task::none()
    }

    /// Rows that fit above the find bar. The saved row count stays put.
    fn rows_on_screen(&self) -> u32 {
        if !self.find_open {
            return self.rows.max(1);
        }
        let inner = self
            .height
            .saturating_sub(geom::vertical_chrome(self.edge, self.frame()) + self.find_height());
        (inner / self.cell().height.max(1)).max(1)
    }

    fn find_extra(&self) -> u32 {
        if self.find_open {
            self.find_height()
        } else {
            0
        }
    }

    fn focus_editor(&mut self) -> Task<Message> {
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            crate::ui::EDITOR_ID.clone(),
        ))
    }

    fn focus_at_cursor(&mut self) -> Task<Message> {
        self.recall_cursor();
        self.focus_editor()
    }

    fn recall_cursor(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.recall_cursor();
        tab.reveal_caret();
    }

    fn hide(&mut self) -> Task<Message> {
        if self.surface.is_none() {
            self.open_when_mapped = false;
            return Task::none();
        }
        let opening = self.wants_open();
        self.open_when_mapped = false;
        self.hold_until = None;
        if !opening && self.slide <= 0.0 && self.motion.is_none() {
            return Task::none();
        }
        self.gesture = None;
        self.capturing = false;
        self.pending_close = None;
        self.find_open = false;
        self.find_submit = false;
        // Leave `settings_window` set so its last frame is still the settings
        // view. Clearing it first paints the drawer into that window.
        let settings = if self.settings_window.is_some() {
            self.close_settings()
        } else {
            Task::none()
        };
        self.persist_documents();
        let slide = if self.slide <= 0.0 && self.motion.is_none() {
            self.park()
        } else {
            self.begin_slide(0.0)
        };
        Task::batch([settings, slide])
    }

    fn begin_slide(&mut self, to: f32) -> Task<Message> {
        if self.hold_until.is_none() && self.motion.as_ref().is_some_and(|motion| motion.to == to) {
            return Task::none();
        }
        if to >= 1.0
            && let Some(mapped) = self.mapped_at
        {
            let ready = mapped + INTRO_HOLD;
            if Instant::now() < ready {
                self.hold_until = Some(ready);
                self.motion = None;
                return Task::none();
            }
        }
        self.hold_until = None;
        if self.motion.is_none() && (self.slide - to).abs() < 0.001 {
            return if to >= 1.0 {
                self.arrive()
            } else {
                self.park()
            };
        }
        self.motion = Some(Motion {
            from: self.slide,
            to,
            started: Instant::now(),
        });
        self.follow_blur();
        if to >= 1.0 {
            Task::batch([
                self.keyboard_for_open(),
                self.hit_region(),
                self.focus_editor(),
            ])
        } else {
            Task::batch([
                self.keyboard(KeyboardInteractivity::None),
                self.hit_region(),
            ])
        }
    }

    fn slide_frame(&mut self) -> Task<Message> {
        if let Some(until) = self.hold_until {
            if Instant::now() < until {
                return Task::none();
            }
            self.hold_until = None;
            return self.begin_slide(1.0);
        }
        let Some(motion) = self.motion else {
            return Task::none();
        };
        let (value, done) = geom::slide_progress(motion.from, motion.to, motion.started.elapsed());
        if !done {
            self.motion = Some(motion);
            self.slide = value;
            return self.hit_region();
        }
        self.slide = motion.to;
        self.motion = None;
        if self.slide >= 1.0 {
            self.arrive()
        } else {
            self.park()
        }
    }

    fn arrive(&mut self) -> Task<Message> {
        self.slide = 1.0;
        self.motion = None;
        self.hold_until = None;
        self.follow_blur();
        Task::batch([
            self.keyboard_for_open(),
            self.hit_region(),
            self.focus_editor(),
        ])
    }

    fn park(&mut self) -> Task<Message> {
        self.slide = 0.0;
        self.motion = None;
        self.hold_until = None;
        crate::blur::set_enabled(false);
        Task::batch([
            self.keyboard(KeyboardInteractivity::None),
            self.hit_region(),
        ])
    }

    fn keyboard_for_open(&self) -> Task<Message> {
        let mode = if self.dialog_open {
            KeyboardInteractivity::None
        } else if self.settings_window.is_some() {
            KeyboardInteractivity::OnDemand
        } else {
            KeyboardInteractivity::Exclusive
        };
        self.keyboard(mode)
    }

    fn select(&mut self, index: usize) -> Task<Message> {
        if index == self.active || index >= self.tabs.len() {
            return Task::none();
        }
        self.persist();
        self.active = index;
        self.focus_at_cursor()
    }

    fn new_tab(&mut self) -> Task<Message> {
        let index = self.next_scratch_index();
        self.tabs.push(Tab::new_scratch(&self.paths.scratch, index));
        self.active = self.tabs.len() - 1;
        self.persist();
        self.focus_at_cursor()
    }

    fn request_close(&mut self, index: usize) -> Task<Message> {
        let Some(tab) = self.tabs.get(index) else {
            return Task::none();
        };
        if tab.needs_close_confirm() {
            self.pending_close = Some(index);
            return Task::none();
        }
        self.pending_close = None;
        self.close_tab(index)
    }

    fn confirm_close(&mut self) -> Task<Message> {
        let Some(index) = self.pending_close.take() else {
            return Task::none();
        };
        self.close_tab(index)
    }

    fn close_tab(&mut self, index: usize) -> Task<Message> {
        if index >= self.tabs.len() {
            return Task::none();
        }
        let mut tab = self.tabs.remove(index);
        if tab.file_path.is_some() && !tab.unreadable() {
            if let Err(error) = tab.flush(false) {
                eprintln!("subtext: save {}: {error}", tab.location_label());
                self.tabs.insert(index, tab);
                return Task::none();
            }
        } else if tab.file_path.is_none() && !tab.unreadable() {
            // Closing a scratch deletes its file. A shared copy that changed
            // underneath this buffer stays where it is.
            if tab.disk_matches_baseline() {
                if let Some(path) = tab.scratch_path.take() {
                    let _ = std::fs::remove_file(path);
                }
            } else {
                eprintln!(
                    "subtext: {} changed on disk, left open",
                    tab.location_label()
                );
                self.tabs.insert(index, tab);
                return Task::none();
            }
        }
        if self.pending_close == Some(index) {
            self.pending_close = None;
        } else if let Some(pending) = self.pending_close.as_mut() {
            if index < *pending {
                *pending -= 1;
            }
        }
        if self.tabs.is_empty() {
            self.active = 0;
        } else if index < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.ensure_scratch();
        self.persist();
        self.focus_at_cursor()
    }

    fn save(&mut self) -> Task<Message> {
        let has_file = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.file_path.as_ref())
            .is_some();
        if has_file {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                if let Err(error) = tab.flush(true) {
                    eprintln!("subtext: save {}: {error}", tab.location_label());
                }
            }
            self.persist();
            Task::none()
        } else {
            self.begin_dialog(true)
        }
    }

    fn begin_dialog(&mut self, save: bool) -> Task<Message> {
        if self.dialog_open {
            return Task::none();
        }
        self.dialog_open = true;
        let map: fn(Option<PathBuf>) -> Message = if save {
            Message::SavedAs
        } else {
            Message::Opened
        };
        // The layer change has to land before the portal opens. An overlay
        // surface, and an exclusive keyboard grab, both sit on top of it.
        self.stack_for_dialog(true)
            .chain(Task::perform(pick_path(save), map))
    }

    fn finish_open(&mut self, path: Option<PathBuf>) -> Task<Message> {
        self.dialog_open = false;
        self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
        let restore = self.stack_for_dialog(false);
        let Some(path) = path else {
            return restore;
        };
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.file_path.as_ref() == Some(&path))
        {
            self.active = index;
            self.persist();
            return restore;
        }
        match Tab::open_file(path) {
            Ok(tab) => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
            }
            Err(error) => eprintln!("subtext: open failed: {error}"),
        }
        self.ensure_scratch();
        self.persist();
        restore
    }

    fn finish_save_as(&mut self, path: Option<PathBuf>) -> Task<Message> {
        self.dialog_open = false;
        self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
        let restore = self.stack_for_dialog(false);
        let Some(path) = path else {
            return restore;
        };
        {
            let Some(tab) = self.tabs.get_mut(self.active) else {
                return restore;
            };
            if matches!(tab.reconcile_disk(), crate::editor::tab::Refresh::Failed) {
                eprintln!(
                    "subtext: save {}: could not reread the file, leaving it untouched",
                    path.display()
                );
                return restore;
            }
            if let Err(error) = tab.write_to(&path) {
                eprintln!("subtext: save {}: {error}", path.display());
                return restore;
            }
            if let Some(scratch) = tab.scratch_path.take() {
                let _ = std::fs::remove_file(scratch);
            }
            tab.title = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            tab.language = crate::editor::Language::from_path(&path);
            tab.file_path = Some(path);
            tab.note_saved();
        }
        self.ensure_scratch();
        self.persist();
        restore
    }

    /// File dialogs are ordinary windows. They stack above `Bottom` and below
    /// `Top`, and they never receive keys while this surface's keyboard mode
    /// is exclusive.
    fn stack_for_dialog(&mut self, open: bool) -> Task<Message> {
        let (layer, keyboard) = if open {
            (Layer::Bottom, KeyboardInteractivity::None)
        } else if self.settings_window.is_some() {
            (Layer::Top, KeyboardInteractivity::OnDemand)
        } else {
            (Layer::Top, KeyboardInteractivity::Exclusive)
        };
        let mut tasks = vec![self.drawer_layer(layer), self.keyboard(keyboard)];
        if let Some(id) = self.settings_window {
            let settings_layer = if open { Layer::Bottom } else { Layer::Overlay };
            tasks.push(Task::done(Message::LayerChange {
                id,
                layer: settings_layer,
            }));
        }
        if !open {
            tasks.push(self.focus_editor());
        }
        Task::batch(tasks)
    }

    fn set_edge(&mut self, edge: Edge) -> Task<Message> {
        if edge == self.edge {
            return Task::none();
        }
        self.remember_active();
        let crossed = edge.horizontal() != self.edge.horizontal();
        self.edge = edge;
        if crossed {
            self.restore_axis();
        }
        if !edge.horizontal() {
            self.span_width = false;
        }
        self.reclamp();
        self.persist();
        // Same anchors either way. Re-sending them makes the compositor
        // reconfigure, which flashes. The in-app slide reads the new edge.
        self.sync_blur();
        self.hit_region()
    }

    fn toggle_line_numbers(&mut self) -> Task<Message> {
        self.line_numbers = !self.line_numbers;
        self.sync_cells();
        self.persist();
        self.focus_editor()
    }

    fn text_pixels(&self) -> f32 {
        self.text_unit.to_pixels(self.text_size)
    }

    fn ui_pixels(&self) -> f32 {
        if self.ui_match_size {
            self.text_pixels()
        } else {
            14.0
        }
    }

    /// Pixels the content sits in from a free edge. The drag target is wider
    /// than this when the border is under [`geom::GRAB`].
    fn grip(&self) -> u32 {
        geom::clamp_border(self.border)
    }

    fn bar_px(&self) -> u32 {
        geom::ui_row(self.ui_pixels())
    }

    fn find_height(&self) -> u32 {
        geom::find_row(self.ui_pixels())
    }

    fn frame(&self) -> geom::Frame {
        geom::Frame {
            pad: geom::editor_pad(self.compact),
            grip: self.grip(),
            bar: self.bar_px(),
        }
    }

    fn set_border(&mut self, px: u32) -> Task<Message> {
        let px = geom::clamp_border(px);
        if px == self.border {
            return Task::none();
        }
        self.border = px;
        self.sync_cells();
        self.persist();
        self.refrost();
        Task::none()
    }

    fn set_text_size(&mut self, size: f32) -> Task<Message> {
        let size = look::clamp_text_size(size, self.text_unit);
        if (size - self.text_size).abs() < f32::EPSILON {
            return Task::none();
        }
        self.text_size = size;
        self.sync_cells();
        self.persist();
        self.refrost();
        Task::none()
    }

    /// Switch the unit without changing how big the glyphs are.
    fn set_text_unit(&mut self, unit: TextUnit) -> Task<Message> {
        if unit == self.text_unit {
            return Task::none();
        }
        let pixels = self.text_pixels();
        self.text_unit = unit;
        self.text_size = look::clamp_text_size(unit.from_pixels(pixels), unit);
        self.sync_cells();
        self.persist();
        Task::none()
    }

    fn toggle_settings(&mut self) -> Task<Message> {
        if self.settings_window.is_some() {
            return self.close_settings();
        }
        self.refresh_screens();
        let (id, open) = crate::message::open_settings(self.output_option());
        self.settings_window = Some(id);
        // Settings is an overlay surface, above this `Top` drawer, so theme
        // changes stay visible. The layer request is a no-op while already open.
        Task::batch([
            open,
            self.drawer_layer(Layer::Top),
            self.keyboard(KeyboardInteractivity::OnDemand),
        ])
    }

    /// Put the caret back in the editor.
    ///
    /// While settings is open the drawer accepts focus on demand. KWin only
    /// activates a layer surface when that flag changes, so a direct switch
    /// to exclusive leaves the keyboard on whatever else was focused. Passing
    /// through none makes the exclusive request a real focus change.
    fn restore_after_settings(&mut self) -> Task<Message> {
        if !self.wants_open() {
            return Task::batch([
                self.drawer_layer(Layer::Top),
                self.keyboard(KeyboardInteractivity::None),
                self.hit_region(),
            ]);
        }
        if self.dialog_open {
            return Task::batch([
                self.drawer_layer(Layer::Bottom),
                self.keyboard(KeyboardInteractivity::None),
            ]);
        }
        let layer = self.drawer_layer(Layer::Top);
        let release = self.keyboard(KeyboardInteractivity::None);
        let grab = self.keyboard(KeyboardInteractivity::Exclusive);
        let focus = self.focus_editor();
        layer.chain(release).chain(grab).chain(focus)
    }

    fn close_settings(&mut self) -> Task<Message> {
        let Some(id) = self.settings_window else {
            return Task::none();
        };
        // Keep the id until `Closed`. Dropping it now makes `view` paint the
        // drawer into the settings window for a frame. The drawer stays on
        // `Top` until that close lands, so this fullscreen surface does not
        // cover settings on the way out. The grace covers the unfocus that
        // closing settings delivers to the drawer.
        self.suppress_unfocus_until = Instant::now() + UNFOCUS_GRACE;
        window::close(id)
    }

    fn drawer_layer(&self, layer: Layer) -> Task<Message> {
        let Some(id) = self.surface else {
            return Task::none();
        };
        Task::done(Message::LayerChange { id, layer })
    }

    fn toggle_full_width(&mut self) -> Task<Message> {
        if self.surface.is_none() {
            return Task::none();
        }
        if self.span_width {
            self.span_width = false;
            self.width = self.restore_width();
            self.reclamp();
            self.persist();
            return self.hit_region();
        }
        self.width_restore = self.width;
        self.span_width = true;
        if let Some(output) = self.output_width {
            self.width = output;
            self.sync_cells();
        }
        self.persist();
        self.hit_region()
    }

    fn dragging(&self, columns_sign: i32, rows_sign: i32, origin: Point) -> Gesture {
        Gesture::Dragging {
            columns_sign,
            rows_sign,
            origin: self.drawer_local(origin),
            start_width: self.width,
            start_height: self.height,
        }
    }

    fn over_tab_bar(&self) -> bool {
        let Some(point) = self.cursor else {
            return false;
        };
        let point = self.drawer_local(point);
        if point.x < 0.0
            || point.y < 0.0
            || point.x > self.width as f32
            || point.y > self.height as f32
        {
            return false;
        }
        let bar = self.bar_px() as f32;
        let grip = self.grip() as f32;
        match self.edge {
            Edge::Bottom => point.y >= self.height as f32 - bar,
            Edge::Top => point.y <= bar,
            Edge::Left | Edge::Right => point.y >= grip && point.y <= grip + bar,
        }
    }

    fn scroll_tabs(&self, delta: f32) -> Task<Message> {
        iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_by(
            crate::ui::TAB_STRIP.clone(),
            iced::advanced::widget::operation::scrollable::AbsoluteOffset { x: delta, y: 0.0 },
        ))
    }

    fn cursor(&mut self, point: Point) -> Task<Message> {
        self.cursor = Some(point);
        match self.gesture {
            Some(Gesture::Armed {
                columns_sign,
                rows_sign,
            }) => {
                self.gesture = Some(self.dragging(columns_sign, rows_sign, point));
                Task::none()
            }
            Some(Gesture::Dragging { .. }) => self.drag_to(point),
            Some(Gesture::Scroll { .. }) => self.drag_scroll(point),
            None => Task::none(),
        }
    }

    fn begin_scroll(&mut self) -> Task<Message> {
        let Some(point) = self.cursor else {
            return Task::none();
        };
        let point = self.drawer_local(point);
        let (top, track_h) = geom::editor_track(
            self.edge,
            self.height,
            self.find_extra(),
            self.grip(),
            self.bar_px(),
        );
        let rows = self.rows_on_screen();
        let line_h = self.cell().height as f32;
        let (line_count, scroll, lead) = self
            .tabs
            .get(self.active)
            .map(|tab| {
                let view = tab.viewport(rows);
                (view.total, view.scroll, view.lead)
            })
            .unwrap_or((1, 0, 0.0));
        let (thumb_h, offset) = geom::thumb_span_px(track_h, line_h, line_count, scroll, lead);
        let local = point.y - top;
        let grab = if local >= offset && local <= offset + thumb_h {
            local - offset
        } else {
            thumb_h / 2.0
        };
        self.gesture = Some(Gesture::Scroll { grab });
        self.drag_scroll(point)
    }

    fn drag_scroll(&mut self, point: Point) -> Task<Message> {
        let Some(Gesture::Scroll { grab }) = self.gesture else {
            return Task::none();
        };
        let point = self.drawer_local(point);
        let (top, track_h) = geom::editor_track(
            self.edge,
            self.height,
            self.find_extra(),
            self.grip(),
            self.bar_px(),
        );
        let rows = self.rows_on_screen();
        let line_h = self.cell().height as f32;
        let line_count = self
            .tabs
            .get(self.active)
            .map(|tab| tab.viewport(rows).total)
            .unwrap_or(1);
        let (thumb_h, _) = geom::thumb_span_px(track_h, line_h, line_count, 0, 0.0);
        let max_scroll = line_count.saturating_sub(rows) as i32;
        let line = geom::scroll_line(point.y - top, grab, track_h, thumb_h, max_scroll);
        self.scroll_active_to(line);
        Task::none()
    }

    fn scroll_active_to(&mut self, line: i32) {
        let rows = self.rows_on_screen();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let view = tab.viewport(rows);
        let max_scroll = (view.total as i32 - rows as i32).max(0);
        let line = line.clamp(0, max_scroll);
        let delta = line - view.scroll;
        if delta == 0 {
            return;
        }
        tab.edit(iced::widget::text_editor::Action::Scroll { lines: delta });
    }

    fn drag_to(&mut self, point: Point) -> Task<Message> {
        let Some(Gesture::Dragging {
            columns_sign,
            rows_sign,
            origin,
            start_width,
            start_height,
        }) = self.gesture
        else {
            return Task::none();
        };
        let Some(output) = self.output_width.zip(self.output_height) else {
            return Task::none();
        };
        let (next_width, next_height) = geom::capture_size(
            self.edge,
            output,
            (start_width, start_height),
            columns_sign,
            rows_sign,
            (origin.x, origin.y),
            (point.x, point.y),
        );
        let (next_width, next_height) = self.clamp_pixels(next_width, next_height);
        if next_width == self.width && next_height == self.height {
            return Task::none();
        }
        self.span_width = false;
        if next_height != self.height {
            self.rows_custom = true;
        }
        self.width = next_width;
        self.height = next_height;
        self.sync_cells();
        self.note_custom_width();
        self.sync_blur();
        // The anchors stay put. Only the opaque rectangle and, on release,
        // the input region change.
        Task::none()
    }

    fn resized(&mut self, id: window::Id, width: f32, height: f32) -> Task<Message> {
        if self.surface != Some(id) {
            return Task::none();
        }
        self.note_output(width, height)
    }

    /// Remember the output and limit clicks to the drawer.
    ///
    /// The layer surface is the whole output. A null input region would
    /// swallow every click on the screen, including the ones that should
    /// dismiss the drawer.
    fn note_output(&mut self, width: f32, height: f32) -> Task<Message> {
        let width = width.round().max(0.0) as u32;
        let height = height.round().max(0.0) as u32;
        // A 0×0 configure is the size we asked for, not the output.
        if width < 2 || height < 2 {
            return Task::none();
        }
        let changed = self.output_width != Some(width) || self.output_height != Some(height);
        self.output_width = Some(width);
        self.output_height = Some(height);
        if changed && !self.capturing {
            // A new display brings its own resolution. Restore the size the
            // user saved, then clamp it to this output.
            self.restore_axis();
        }
        if self.span_width && self.edge.horizontal() {
            self.width = width;
        }
        self.reclamp();
        self.sync_blur();
        self.hit_region()
    }

    fn press_outside(&mut self, id: window::Id) -> Task<Message> {
        if !self.click_away
            || self.surface != Some(id)
            || self.settings_window.is_some()
            || self.dialog_open
            || self.gesture.is_some()
            || self.capturing
            || self.output_width.is_none()
            || Instant::now() < self.suppress_unfocus_until
        {
            return Task::none();
        }
        let Some(point) = self.cursor else {
            return Task::none();
        };
        if self.inside_drawer(point) {
            Task::none()
        } else {
            self.hide()
        }
    }

    fn inside_drawer(&self, point: Point) -> bool {
        let point = self.drawer_local(point);
        point.x >= -1.0
            && point.y >= -1.0
            && point.x <= self.width as f32 + 1.0
            && point.y <= self.height as f32 + 1.0
    }

    /// Pointer position in drawer pixels. Events arrive in output pixels.
    fn drawer_local(&self, point: Point) -> Point {
        let Some(output) = self.output_width.zip(self.output_height) else {
            return point;
        };
        let (x, y) = geom::slide_origin(self.edge, output, (self.width, self.height), self.slide);
        Point::new(point.x - x as f32, point.y - y as f32)
    }

    /// Hit target: the drawer, or the whole output while a grip is held.
    ///
    /// The default region is the entire surface. Until the output size is
    /// known the compositor would otherwise deliver every click on the screen.
    fn hit_region(&self) -> Task<Message> {
        let Some(id) = self.surface else {
            return Task::none();
        };
        let Some((output_w, output_h)) = self.output_width.zip(self.output_height) else {
            return Task::none();
        };
        // A parked drawer is not drawn. Its old rectangle must not keep eating
        // clicks on the output after the plate has left.
        let rect = if self.capturing && self.wants_open() {
            Some((0, 0, output_w as i32, output_h as i32))
        } else if !self.drawer_painted() {
            None
        } else {
            let origin = geom::slide_origin(
                self.edge,
                (output_w, output_h),
                (self.width, self.height),
                self.slide,
            );
            geom::output_intersection(origin, (self.width, self.height), (output_w, output_h))
        };
        if self.blur && self.drawer_painted() {
            self.sync_blur();
        }
        Task::done(Message::SetInputRegion {
            id,
            callback: iced_layershell::actions::ActionCallback::new(move |region| {
                // The handler only clears the current window rectangle, and a
                // drawer can sit away from the origin. Clear a wide area first.
                // Adding nothing leaves the region empty, which is click-through.
                region.subtract(-16_384, -16_384, 32_768, 32_768);
                if let Some((x, y, width, height)) = rect
                    && width > 0
                    && height > 0
                {
                    region.add(x, y, width, height);
                }
            }),
        })
    }

    fn cell(&self) -> Cell {
        Cell::for_text_size(self.text_pixels())
    }

    fn gutter(&self) -> u32 {
        if !self.line_numbers {
            return 0;
        }
        let lines = self.tabs.get(self.active).map_or(1, |tab| tab.line_count());
        geom::gutter_width(
            self.cell(),
            gutter_digits(lines, self.compact),
            geom::editor_pad(self.compact),
        )
    }

    fn ensure_pixels(&mut self) {
        if self.width == 0 || self.height == 0 {
            let (width, height) = pixel_size(
                self.edge,
                self.columns,
                self.rows,
                self.cell(),
                self.gutter(),
                self.frame(),
            );
            if self.width == 0 {
                self.width = width;
            }
            if self.height == 0 {
                self.height = height;
            }
        }
        if self.width_restore == 0 {
            self.width_restore = self.width;
        }
        self.reclamp();
    }

    fn reclamp(&mut self) {
        let (width, height) = self.clamp_pixels(self.width, self.height);
        self.width = width;
        self.height = height;
        self.sync_cells();
    }

    fn sync_cells(&mut self) {
        let cell = self.cell();
        let inner_w = self
            .width
            .saturating_sub(geom::horizontal_chrome(self.edge, self.frame()) + self.gutter());
        let inner_h = self
            .height
            .saturating_sub(geom::vertical_chrome(self.edge, self.frame()));
        self.columns = (inner_w / cell.width.max(1)).max(1);
        self.rows = (inner_h / cell.height.max(1)).max(1);
    }

    fn clamp_pixels(&self, width: u32, height: u32) -> (u32, u32) {
        let (min_w, min_h) = self.min_pixels();
        let mut width = width.max(min_w);
        let mut height = height.max(min_h);
        if let Some(max_w) = self.output_width {
            width = width.min(max_w.max(min_w));
        }
        if let Some(max_h) = self.output_height {
            height = height.min(max_h.max(min_h));
        }
        (width, height)
    }

    fn min_pixels(&self) -> (u32, u32) {
        pixel_size(
            self.edge,
            geom::MIN_COLS,
            geom::MIN_ROWS,
            self.cell(),
            self.gutter(),
            self.frame(),
        )
    }

    fn restore_width(&self) -> u32 {
        let fallback = pixel_size(
            self.edge,
            geom::DEFAULT_COLS,
            self.rows,
            self.cell(),
            self.gutter(),
            self.frame(),
        )
        .0;
        let restore = if self.width_restore < self.min_pixels().0 {
            fallback
        } else {
            self.width_restore
        };
        match self.output_width {
            Some(output) if restore + 2 >= output => fallback.min(output),
            Some(output) => restore.min(output),
            None => restore,
        }
    }

    fn note_custom_width(&mut self) {
        if self
            .output_width
            .is_none_or(|output| self.width + 4 < output)
        {
            self.width_restore = self.width;
        }
    }

    fn keyboard(&self, mode: KeyboardInteractivity) -> Task<Message> {
        let Some(id) = self.surface else {
            return Task::none();
        };
        Task::done(Message::KeyboardInteractivityChange {
            id,
            keyboard_interactivity: mode,
        })
    }

    fn layer_settings(&self) -> NewLayerShellSettings {
        let (anchor, size, margin) = self.shell_layout();
        // `Top` stays under overlay clients such as Spectacle. Settings is the
        // surface that uses `Overlay`. A file dialog is an ordinary window, so
        // both surfaces drop to `Bottom` while one is open.
        let layer = if self.dialog_open {
            Layer::Bottom
        } else {
            Layer::Top
        };
        NewLayerShellSettings {
            size: Some(size),
            layer,
            anchor,
            exclusive_zone: Some(0),
            margin: Some(margin),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: self.output_option(),
            events_transparent: false,
            namespace: Some("subtext".into()),
        }
    }

    fn output_option(&self) -> OutputOption {
        match &self.bound_output {
            Some(name) if !name.is_empty() => OutputOption::OutputName(name.clone()),
            _ => OutputOption::Active,
        }
    }

    fn refresh_screens(&mut self) {
        let found = crate::output::list();
        if !found.is_empty() {
            self.screens = found;
        }
    }

    /// Where the next surface should be created. `None` leaves the choice to
    /// the compositor.
    fn resolved_output(&mut self) -> Option<String> {
        let active = if self.follow_cursor {
            crate::output::active_name()
        } else {
            None
        };
        let choice = crate::output::choose(
            self.follow_cursor,
            &self.output_name,
            &self.screens,
            active.as_deref(),
        );
        if !self.follow_cursor
            && let Some(name) = choice.clone()
        {
            let saved_gone = !self.output_name.is_empty()
                && !self
                    .screens
                    .iter()
                    .any(|screen| screen.name == self.output_name);
            if saved_gone {
                self.output_name = name;
                self.persist();
            }
        }
        choice
    }

    /// Close the current surface, if there is one, and open another on `name`.
    fn retarget(&mut self, name: Option<String>) -> Task<Message> {
        self.bound_output = name;
        if let Some(id) = self.surface {
            self.reopen_drawer = true;
            return window::close(id);
        }
        self.open_surface()
    }

    fn toggle_follow(&mut self) -> Task<Message> {
        self.follow_cursor = !self.follow_cursor;
        self.persist();
        if !self.wants_open() {
            if self.follow_cursor {
                return Task::none();
            }
            self.refresh_screens();
            let target = self.resolved_output();
            if self.bound_output == target {
                return Task::none();
            }
            return self.retarget(target);
        }
        self.refresh_screens();
        let target = self.resolved_output();
        if self.bound_output == target {
            return Task::none();
        }
        self.open_when_mapped = true;
        self.retarget(target)
    }

    fn show_on(&mut self, name: String) -> Task<Message> {
        self.follow_cursor = false;
        self.output_name = name;
        self.persist();
        self.refresh_screens();
        let target = self.resolved_output();
        if self.bound_output == target {
            return Task::none();
        }
        if self.wants_open() {
            self.open_when_mapped = true;
        }
        self.retarget(target)
    }

    /// Always the whole output. Size 0 with every edge anchored asks the
    /// compositor for the output size and then leaves it there, so a grab
    /// does not resize the surface. The drawer is drawn inside that surface
    /// and slides by moving that rectangle.
    fn shell_layout(&self) -> (Anchor, (u32, u32), (i32, i32, i32, i32)) {
        let anchor = Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right;
        (anchor, (0, 0), (0, 0, 0, 0))
    }

    fn remember_active(&mut self) {
        if self.edge.horizontal() {
            if self.span_width {
                self.across_width = self.width;
            } else {
                self.across_width =
                    geom::preferred_size(self.across_width, self.width, self.output_width);
            }
            self.across_height =
                geom::preferred_size(self.across_height, self.height, self.output_height);
            self.across_restore =
                geom::preferred_size(self.across_restore, self.width_restore, self.output_width);
            self.across_span = self.span_width;
        } else {
            self.side_width = geom::preferred_size(self.side_width, self.width, self.output_width);
            self.side_height =
                geom::preferred_size(self.side_height, self.height, self.output_height);
        }
    }

    fn restore_axis(&mut self) {
        if self.edge.horizontal() {
            if self.across_width == 0 || self.across_height == 0 {
                let (width, height) = pixel_size(
                    Edge::Bottom,
                    geom::DEFAULT_COLS,
                    self.rows.max(geom::MIN_ROWS),
                    self.cell(),
                    self.gutter(),
                    self.frame(),
                );
                self.width = width;
                self.height = height;
                self.width_restore = width;
                self.span_width = false;
            } else {
                self.width = self.across_width;
                self.height = self.across_height;
                self.width_restore = if self.across_restore == 0 {
                    self.across_width
                } else {
                    self.across_restore
                };
                self.span_width = self.across_span;
            }
        } else if self.side_width == 0 || self.side_height == 0 {
            let (width, height) =
                pixel_size(Edge::Left, 64, 28, self.cell(), self.gutter(), self.frame());
            self.width = width;
            self.height = height;
            self.width_restore = width;
            self.span_width = false;
        } else {
            self.width = self.side_width;
            self.height = self.side_height;
            self.span_width = false;
        }
    }

    /// Keep the frost on the part of the plate that is still on the output.
    /// Once that part is gone, drop the region. Leaving the previous rectangle
    /// up shows frost the drawer no longer covers.
    /// Publish the plate rectangle again. Chrome changes do not move it, and
    /// KWin will not notice a new buffer until the blur object is replaced.
    fn refrost(&self) {
        self.sync_blur();
        crate::blur::refresh_drawer();
    }

    fn sync_blur(&self) {
        if !self.blur || !self.drawer_painted() {
            return;
        }
        let Some(output) = self.output_width.zip(self.output_height) else {
            return;
        };
        let size = (self.width, self.height);
        let origin = geom::slide_origin(self.edge, output, size, self.slide);
        match geom::output_intersection(origin, size, output) {
            Some((x, y, width, height)) => {
                crate::blur::set_drawer((x, y), (width as u32, height as u32));
            }
            None => crate::blur::clear_drawer(),
        }
    }

    fn follow_blur(&self) {
        if self.blur && self.drawer_painted() {
            self.sync_blur();
            crate::blur::set_enabled(true);
        } else {
            crate::blur::set_enabled(false);
        }
    }

    /// User opacity once the frost matches the plate. Until the rectangle has
    /// been installed the plate stays solid, so the first map cannot frost the
    /// empty margin or show the desktop through an unblurred plate.
    fn plate_opacity(&self) -> f32 {
        if self.blur && !crate::blur::drawer_ready() {
            1.0
        } else {
            self.opacity
        }
    }

    /// Reread every open file. Called when the drawer is shown, before the
    /// user keeps typing into a copy that another computer already replaced.
    fn reconcile_tabs(&mut self) {
        let gutter = self.gutter();
        for tab in &mut self.tabs {
            let _ = tab.reconcile_disk();
        }
        if self.gutter() != gutter {
            self.reclamp();
        }
    }

    /// Settings, tab choice, and cursor. Dirty scratch buffers are written too,
    /// so a restart can restore them. Open files are left alone.
    fn persist(&mut self) {
        self.store(false);
    }

    /// `persist`, and also write dirty files. Used when the drawer hides or
    /// the surface closes, so dismissing it does not drop unsaved edits.
    fn persist_documents(&mut self) {
        self.store(true);
    }

    fn store(&mut self, files: bool) {
        self.note_editor_scroll();
        self.remember_active();
        for tab in &mut self.tabs {
            let is_file = tab.file_path.is_some();
            if is_file && !files {
                continue;
            }
            if !tab.dirty {
                continue;
            }
            if let Err(error) = tab.flush(false) {
                eprintln!("subtext: save {}: {error}", tab.location_label());
            }
        }
        let snapshot = Snapshot {
            anchor: self.edge,
            columns: self.columns,
            rows: self.rows,
            active: self.active,
            tabs: self.tabs.iter().map(Tab::record).collect(),
            text_size: self.text_size,
            text_unit: self.text_unit,
            font: self.font,
            ui_font: self.ui_font,
            ui_match_size: self.ui_match_size,
            border: self.border,
            theme: self.theme,
            line_numbers: self.line_numbers,
            word_wrap: self.word_wrap,
            compact: self.compact,
            tab_stop: self.tab_stop,
            width: self.width,
            height: self.height,
            width_restore: self.width_restore,
            span_width: self.span_width,
            across_width: self.across_width,
            across_height: self.across_height,
            across_width_restore: self.across_restore,
            across_span: self.across_span,
            side_width: self.side_width,
            side_height: self.side_height,
            opacity: self.opacity,
            blur: self.blur,
            remember_indent: self.remember_indent,
            hinting: self.hinting,
            click_away: self.click_away,
            follow_cursor: self.follow_cursor,
            output_name: self.output_name.clone(),
        };
        if let Err(error) = state::write(&self.paths, &snapshot) {
            eprintln!("subtext: could not write state: {error}");
        }
    }

    fn backspace(&mut self) -> Task<Message> {
        let unit = self.tab_stop.unit();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if !tab.eat_indent(unit) {
                tab.edit(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Backspace,
                ));
            }
        }
        Task::none()
    }

    fn break_line(&mut self) -> Task<Message> {
        let unit = self.remember_indent.then(|| self.tab_stop.unit());
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.break_line(unit);
        }
        Task::none()
    }

    fn indent_active(&mut self, outdent: bool) -> Task<Message> {
        let unit = self.tab_stop.unit();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if outdent {
                tab.unindent(unit);
            } else {
                tab.indent(unit);
            }
        }
        Task::none()
    }

    fn history(&mut self, undo: bool) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if undo {
                tab.undo();
            } else {
                tab.redo();
            }
        }
        Task::none()
    }

    /// Keep the saved scroll on the buffer line the editor is showing.
    fn note_editor_scroll(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.note_top_line();
        }
    }

    /// A scratch tab stays open beside any real files.
    fn ensure_scratch(&mut self) {
        let has_scratch = self
            .tabs
            .iter()
            .any(|tab| tab.file_path.is_none() && tab.scratch_path.is_some());
        if has_scratch {
            return;
        }
        let index = self.next_scratch_index();
        self.tabs.push(Tab::new_scratch(&self.paths.scratch, index));
        if self.tabs.len() == 1 {
            self.active = 0;
        }
    }

    fn next_scratch_index(&self) -> u32 {
        let mut index = 1u32;
        loop {
            let path = self.paths.scratch.join(format!("scratch_{index}.txt"));
            let open = self
                .tabs
                .iter()
                .any(|tab| tab.scratch_path.as_deref() == Some(path.as_path()));
            if !open && !path.exists() {
                return index;
            }
            index += 1;
            if index > 100_000 {
                return index;
            }
        }
    }
}

pub fn settings(connection: Option<iced_layershell::reexport::WithConnection>) -> Settings {
    let mut settings = Settings {
        id: Some("subtext".into()),
        default_font: Font::MONOSPACE,
        default_text_size: iced::Pixels(geom::TEXT_SIZE),
        layer_settings: LayerShellSettings {
            anchor: Anchor::Bottom,
            layer: Layer::Top,
            exclusive_zone: 0,
            size: None,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::Background,
            events_transparent: false,
        },
        ..Settings::default()
    };
    if let Some(connection) = connection {
        settings.with_connection = Some(connection);
    }
    settings
}

fn clear_output() -> Element<'static, Message> {
    iced::widget::space()
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

fn slide_frames() -> impl iced::futures::Stream<Item = ()> + Send + 'static {
    iced::stream::channel(1, async |sender| {
        std::thread::spawn(move || pump_frames(sender));
        std::future::pending::<()>().await;
    })
}

fn pump_frames(mut sender: iced::futures::channel::mpsc::Sender<()>) {
    loop {
        std::thread::sleep(Duration::from_millis(16));
        match sender.try_send(()) {
            Ok(()) => {}
            Err(error) if error.is_full() => {}
            Err(_) => break,
        }
    }
}

fn clamp_opacity(value: f32) -> f32 {
    if !value.is_finite() {
        return 1.0;
    }
    ((value.clamp(0.2, 1.0) * 20.0).round() / 20.0).clamp(0.2, 1.0)
}

/// Vertical wheel over the tab strip, in horizontal pixels. A mostly
/// horizontal wheel is left for the strip's own scrollbar.
fn vertical_wheel(delta: iced::mouse::ScrollDelta) -> Option<f32> {
    let (x, y) = match delta {
        iced::mouse::ScrollDelta::Lines { x, y } => (x * 60.0, y * 60.0),
        iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
    };
    if y.abs() <= x.abs() { None } else { Some(-y) }
}

fn on_event(event: Event, status: iced::event::Status, window: window::Id) -> Option<Message> {
    use iced::keyboard::key;
    use iced::keyboard::{Event as KeyEvent, Key};
    use iced::mouse::{Button, Event as MouseEvent};
    use iced::window::Event as WindowEvent;

    match event {
        Event::Mouse(MouseEvent::CursorMoved { position }) => {
            Some(Message::Cursor(window, position))
        }
        Event::Mouse(MouseEvent::WheelScrolled { delta }) => {
            vertical_wheel(delta).map(|delta| Message::ScrollTabs { window, delta })
        }
        Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
            Some(Message::PointerPressed(window))
        }
        Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => Some(Message::DragEnd),
        Event::Window(WindowEvent::Opened { size, .. }) => {
            Some(Message::SurfaceOpened(window, size.width, size.height))
        }
        Event::Window(WindowEvent::Unfocused) => Some(Message::Unfocused(window)),
        Event::Window(WindowEvent::Resized(size)) => {
            Some(Message::Resized(window, size.width, size.height))
        }
        Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => match key.as_ref() {
            Key::Named(key::Named::Escape) if !modifiers.command() && !modifiers.alt() => {
                Some(Message::Dismiss)
            }
            Key::Character("s") if modifiers.command() => Some(Message::Save),
            Key::Character("n") if modifiers.command() => Some(Message::NewTab),
            Key::Character("w") if modifiers.command() => Some(Message::CloseActive),
            Key::Character("o") if modifiers.command() => Some(Message::Open),
            Key::Character("z") if modifiers.command() && !modifiers.alt() => {
                Some(if modifiers.shift() {
                    Message::Redo
                } else {
                    Message::Undo
                })
            }
            Key::Character("y") if modifiers.command() && !modifiers.alt() => Some(Message::Redo),
            Key::Character(character)
                if modifiers.command()
                    && !modifiers.alt()
                    && character.eq_ignore_ascii_case("f") =>
            {
                Some(Message::Find)
            }
            Key::Named(key::Named::F3) if !modifiers.command() && !modifiers.alt() => {
                Some(if modifiers.shift() {
                    Message::FindPrevious
                } else {
                    Message::FindNext
                })
            }
            Key::Named(key::Named::Enter)
                if status == iced::event::Status::Captured
                    && !modifiers.command()
                    && !modifiers.alt() =>
            {
                Some(Message::FindEnter {
                    shift: modifiers.shift(),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

async fn pick_path(save: bool) -> Option<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Let the layer drop below ordinary windows before the portal maps.
        std::thread::sleep(Duration::from_millis(100));
        let dialog = rfd::FileDialog::new();
        let picked = if save {
            dialog.set_title("Save as").save_file()
        } else {
            dialog.set_title("Open").pick_file()
        };
        let _ = tx.send(picked);
    });
    rx.recv().ok().flatten()
}

/// Close the active tab when the shortcut used `usize::MAX`.
pub fn run() -> iced_layershell::Result {
    let connection = crate::slide::prepare().map(iced_layershell::reexport::WithConnection::from);
    iced_layershell::daemon(App::boot, "subtext", App::update, App::view)
        .settings(settings(connection))
        .theme(|state: &App, _id| Some(state.theme()))
        .style(App::appearance)
        .subscription(App::subscription)
        .run()
}

/// Digits the gutter reserves. Compact shrinks to the line count, with a floor
/// of two. The ordinary gutter stays four wide until the file needs more.
fn gutter_digits(lines: u32, compact: bool) -> u32 {
    let digits = lines.max(1).to_string().len() as u32;
    if compact {
        digits.max(2)
    } else {
        digits.max(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Language;
    use crate::editor::tab::Record;

    fn file_only(file: PathBuf) -> Snapshot {
        Snapshot {
            anchor: Edge::Bottom,
            columns: 80,
            rows: 12,
            active: 0,
            tabs: vec![Record {
                id: "file".into(),
                title: "note.txt".into(),
                file_path: Some(file),
                scratch_path: None,
                language: Language::Plain,
                cursor_line: 0,
                cursor_column: 0,
                scroll_lines: 0,
            }],
            text_size: 16.0,
            text_unit: TextUnit::Pt,
            font: crate::look::FontChoice::SYSTEM,
            ui_font: UiFont::MatchEditor,
            ui_match_size: false,
            border: 1,
            theme: crate::look::ThemeChoice::Dracula,
            line_numbers: true,
            word_wrap: false,
            compact: false,
            width: 800,
            height: 400,
            width_restore: 640,
            span_width: false,
            across_width: 800,
            across_height: 400,
            across_width_restore: 640,
            across_span: false,
            side_width: 0,
            side_height: 0,
            opacity: 1.0,
            blur: false,
            tab_stop: TabStop::Spaces4,
            remember_indent: false,
            hinting: true,
            click_away: true,
            follow_cursor: false,
            output_name: String::new(),
        }
    }

    #[test]
    fn a_file_tab_still_has_a_scratch_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        std::fs::create_dir_all(&paths.scratch).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let app = App::from_snapshot(paths, file_only(file));
        assert!(app.tabs.iter().any(|tab| tab.file_path.is_some()));
        assert!(
            app.tabs
                .iter()
                .any(|tab| tab.file_path.is_none() && tab.scratch_path.is_some())
        );
        assert_eq!(app.active, 0);
    }

    #[test]
    fn persisting_state_does_not_rewrite_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        std::fs::create_dir_all(&paths.scratch).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let mut app = App::from_snapshot(paths, file_only(file.clone()));
        app.tabs[0].content = iced::widget::text_editor::Content::with_text("changed");
        app.tabs[0].dirty = true;
        app.opacity = 0.5;
        app.persist();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
        let _ = app.save();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "changed");
    }

    #[test]
    fn persisting_state_writes_a_dirty_scratch_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        let scratch = paths.scratch.join("scratch_1.txt");
        std::fs::create_dir_all(&paths.scratch).unwrap();
        std::fs::write(&scratch, "old").unwrap();
        let mut snapshot = file_only(dir.path().join("missing.txt"));
        snapshot.tabs[0].file_path = None;
        snapshot.tabs[0].scratch_path = Some(scratch.clone());
        snapshot.tabs[0].title = "Scratch 1".into();
        let mut app = App::from_snapshot(paths, snapshot);
        app.tabs[0].content = iced::widget::text_editor::Content::with_text("kept");
        app.tabs[0].dirty = true;
        app.persist();
        assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "kept");
        std::fs::write(&scratch, "tampered").unwrap();
        app.persist();
        assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "tampered");
    }

    #[test]
    fn hiding_writes_a_dirty_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        std::fs::create_dir_all(&paths.scratch).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let mut app = App::from_snapshot(paths, file_only(file.clone()));
        app.tabs[0].content = iced::widget::text_editor::Content::with_text("saved");
        app.tabs[0].dirty = true;
        app.persist_documents();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "saved");
    }

    #[test]
    fn save_as_keeps_the_scratch_when_the_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        let scratch = paths.scratch.join("scratch_1.txt");
        std::fs::create_dir_all(&paths.scratch).unwrap();
        std::fs::write(&scratch, "keep me").unwrap();
        let mut snapshot = file_only(dir.path().join("unused.txt"));
        snapshot.tabs[0].file_path = None;
        snapshot.tabs[0].scratch_path = Some(scratch.clone());
        let mut app = App::from_snapshot(paths, snapshot);
        let dest = dir.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        let _ = app.finish_save_as(Some(dest));
        assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "keep me");
        assert!(scratch.exists());
        assert!(app.tabs[0].file_path.is_none());
        assert_eq!(app.tabs[0].scratch_path.as_deref(), Some(scratch.as_path()));
    }

    #[test]
    fn save_as_drops_the_scratch_after_the_new_file_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        let scratch = paths.scratch.join("scratch_1.txt");
        std::fs::create_dir_all(&paths.scratch).unwrap();
        std::fs::write(&scratch, "keep me").unwrap();
        let mut snapshot = file_only(dir.path().join("unused.txt"));
        snapshot.tabs[0].file_path = None;
        snapshot.tabs[0].scratch_path = Some(scratch.clone());
        let mut app = App::from_snapshot(paths, snapshot);
        let dest = dir.path().join("note.md");
        let _ = app.finish_save_as(Some(dest.clone()));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "keep me");
        assert!(!scratch.exists());
        assert_eq!(app.tabs[0].file_path.as_deref(), Some(dest.as_path()));
        assert!(app.tabs[0].scratch_path.is_none());
    }

    #[test]
    fn a_wider_gutter_shrinks_the_column_count() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        std::fs::create_dir_all(&paths.scratch).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let mut snapshot = file_only(file);
        snapshot.compact = true;
        snapshot.width = 800;
        snapshot.height = 400;
        let mut app = App::from_snapshot(paths, snapshot);
        let columns = app.columns;
        let gutter = app.gutter();
        app.tabs[0].content = iced::widget::text_editor::Content::with_text(&"x\n".repeat(10_000));
        assert!(app.gutter() > gutter);
        app.reclamp();
        assert!(app.columns < columns);
    }

    fn scratch_app(dir: &std::path::Path, text: &str) -> (std::path::PathBuf, App) {
        let paths = Paths::under(dir.join("subtext"));
        let scratch = paths.scratch.join("scratch_1.txt");
        std::fs::create_dir_all(&paths.scratch).unwrap();
        std::fs::write(&scratch, text).unwrap();
        let mut snapshot = file_only(dir.join("unused.txt"));
        snapshot.tabs[0].file_path = None;
        snapshot.tabs[0].scratch_path = Some(scratch.clone());
        snapshot.tabs[0].title = "Scratch 1".into();
        (scratch, App::from_snapshot(paths, snapshot))
    }

    #[test]
    fn opening_reloads_a_scratch_another_computer_saved() {
        let dir = tempfile::tempdir().unwrap();
        let (scratch, mut app) = scratch_app(dir.path(), "one\n");
        std::fs::write(&scratch, "two\n").unwrap();
        app.reconcile_tabs();
        assert_eq!(app.tabs[0].text(), "two\n");
        assert!(!app.tabs[0].dirty);
    }

    #[test]
    fn closing_a_scratch_does_not_delete_a_newer_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (scratch, mut app) = scratch_app(dir.path(), "one\n");
        std::fs::write(&scratch, "two\n").unwrap();
        let _ = app.close_tab(0);
        assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "two\n");
        assert_eq!(app.tabs[0].text(), "two\n");
    }

    #[test]
    fn hiding_merges_a_shared_scratch_instead_of_replacing_it() {
        let dir = tempfile::tempdir().unwrap();
        let (scratch, mut app) = scratch_app(dir.path(), "a\n");
        app.tabs[0].content = iced::widget::text_editor::Content::with_text("a\nlocal\n");
        app.tabs[0].dirty = true;
        std::fs::write(&scratch, "remote\na\n").unwrap();
        app.persist_documents();
        assert_eq!(
            std::fs::read_to_string(&scratch).unwrap(),
            "remote\na\nlocal\n"
        );
    }

    #[test]
    fn hiding_does_not_write_a_conflict_over_the_shared_file() {
        let dir = tempfile::tempdir().unwrap();
        let (scratch, mut app) = scratch_app(dir.path(), "a\n");
        app.tabs[0].content = iced::widget::text_editor::Content::with_text("local\n");
        app.tabs[0].dirty = true;
        std::fs::write(&scratch, "remote\n").unwrap();
        app.persist_documents();
        assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "remote\n");
        assert!(app.tabs[0].conflicted());
        assert!(
            app.tabs[0]
                .text()
                .contains("<<<<<<< subtext (this computer)")
        );
    }

    #[test]
    fn gutter_digits_grow_past_four() {
        assert_eq!(gutter_digits(1, true), 2);
        assert_eq!(gutter_digits(100, true), 3);
        assert_eq!(gutter_digits(1, false), 4);
        assert_eq!(gutter_digits(9_999, false), 4);
        assert_eq!(gutter_digits(10_000, false), 5);
        assert_eq!(gutter_digits(10_000, true), 5);
    }
}

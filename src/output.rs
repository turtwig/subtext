//! Displays the drawer can slide out on.
//!
//! A short-lived Wayland connection reads each output's name and logical
//! size. Logical sizes are what a layer surface is configured with, so two
//! panels with different resolutions stay in the same coordinate space.
//! The primary display is the first name in `kde_output_order_v1`, or the
//! output at logical `(0, 0)` when that protocol is absent. Follow-cursor
//! asks KWin which output is active. That is the screen the pointer last
//! interacted with. Other compositors fall back to the primary display.

use std::collections::HashMap;
use std::fmt;
use std::process::Command;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_output;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1;
use wayland_protocols_plasma::output_order::v1::client::kde_output_order_v1;

/// One connected display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub name: String,
    pub description: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

impl Screen {
    /// Text for the settings dropdown.
    pub fn label(&self) -> String {
        let mut label = if self.primary {
            format!("Primary · {}", self.name)
        } else {
            self.name.clone()
        };
        if self.width > 0 && self.height > 0 {
            label.push_str(&format!(" · {}×{}", self.width, self.height));
        }
        if !self.description.is_empty() && self.description != self.name {
            label.push_str(" · ");
            label.push_str(&self.description);
        }
        label
    }
}

/// One row of the display dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenChoice {
    pub name: String,
    pub label: String,
}

impl fmt::Display for ScreenChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.label)
    }
}

pub fn choices(screens: &[Screen]) -> Vec<ScreenChoice> {
    let mut screens = screens.to_vec();
    screens.sort_by(|left, right| {
        right
            .primary
            .cmp(&left.primary)
            .then(left.y.cmp(&right.y))
            .then(left.x.cmp(&right.x))
            .then(left.name.cmp(&right.name))
    });
    screens
        .into_iter()
        .map(|screen| ScreenChoice {
            label: screen.label(),
            name: screen.name,
        })
        .collect()
}

/// The output the drawer should bind.
///
/// An empty saved name means the primary display. A saved name that is no
/// longer connected also returns the primary. Follow-cursor uses `active`
/// when that name is still connected.
pub fn choose(
    follow: bool,
    saved: &str,
    screens: &[Screen],
    active: Option<&str>,
) -> Option<String> {
    let primary = screens
        .iter()
        .find(|screen| screen.primary)
        .or_else(|| screens.first())?;
    if follow {
        if let Some(name) = active
            && screens.iter().any(|screen| screen.name == name)
        {
            return Some(name.to_string());
        }
        return Some(primary.name.clone());
    }
    if !saved.is_empty() && screens.iter().any(|screen| screen.name == saved) {
        return Some(saved.to_string());
    }
    Some(primary.name.clone())
}

/// Names, logical geometry, and which display is primary.
///
/// An empty list means the connection could not be opened. Callers keep the
/// output they already have.
pub fn list() -> Vec<Screen> {
    let Ok(connection) = Connection::connect_to_env() else {
        return Vec::new();
    };
    let Ok((globals, mut queue)) = registry_queue_init::<Query>(&connection) else {
        return Vec::new();
    };
    let mut query = Query::default();
    let handle = queue.handle();
    let registry = globals.registry();
    let mut outputs = Vec::new();
    for global in globals.contents().clone_list() {
        if global.interface != "wl_output" {
            continue;
        }
        let version = global.version.clamp(1, 4);
        let output: wl_output::WlOutput = registry.bind(global.name, version, &handle, global.name);
        query.heads.insert(global.name, Head::default());
        outputs.push((global.name, output));
    }
    if let Ok(manager) = globals.bind::<ZxdgOutputManagerV1, _, _>(&handle, 1..=3, ()) {
        for (name, output) in &outputs {
            query
                .xdg
                .push(manager.get_xdg_output(output, &handle, *name));
        }
        query.manager = Some(manager);
    }
    if let Ok(order) =
        globals.bind::<kde_output_order_v1::KdeOutputOrderV1, _, _>(&handle, 1..=1, ())
    {
        query.order_proxy = Some(order);
    }
    query.outputs = outputs.into_iter().map(|(_, output)| output).collect();
    if queue.roundtrip(&mut query).is_err() {
        return Vec::new();
    }
    let _ = queue.roundtrip(&mut query);
    screens_from(query)
}

/// KWin's active output. `None` when the compositor does not offer it.
pub fn active_name() -> Option<String> {
    let output = Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.kde.KWin",
            "/KWin",
            "org.kde.KWin",
            "activeOutputName",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_busctl_string(&String::from_utf8_lossy(&output.stdout))
}

fn parse_busctl_string(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("s ")?.trim();
    let rest = rest.strip_prefix('"')?.strip_suffix('"')?;
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

#[derive(Default)]
struct Head {
    name: String,
    description: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    wl_name: String,
    wl_description: String,
    mode_width: i32,
    mode_height: i32,
}

#[derive(Default)]
#[allow(dead_code)]
struct Query {
    heads: HashMap<u32, Head>,
    outputs: Vec<wl_output::WlOutput>,
    xdg: Vec<zxdg_output_v1::ZxdgOutputV1>,
    manager: Option<ZxdgOutputManagerV1>,
    order_proxy: Option<kde_output_order_v1::KdeOutputOrderV1>,
    order: Vec<String>,
    order_open: bool,
}

fn screens_from(query: Query) -> Vec<Screen> {
    let mut screens: Vec<Screen> = query
        .heads
        .into_values()
        .filter_map(|head| {
            let name = if !head.name.is_empty() {
                head.name
            } else {
                head.wl_name
            };
            if name.is_empty() {
                return None;
            }
            let description = if !head.description.is_empty() {
                head.description
            } else {
                head.wl_description
            };
            let (width, height) = if head.width > 0 && head.height > 0 {
                (head.width, head.height)
            } else {
                (head.mode_width, head.mode_height)
            };
            Some(Screen {
                name,
                description,
                x: head.x,
                y: head.y,
                width,
                height,
                primary: false,
            })
        })
        .collect();
    mark_primary(&mut screens, &query.order);
    screens.sort_by(|left, right| {
        right
            .primary
            .cmp(&left.primary)
            .then(left.y.cmp(&right.y))
            .then(left.x.cmp(&right.x))
            .then(left.name.cmp(&right.name))
    });
    screens
}

fn mark_primary(screens: &mut [Screen], order: &[String]) {
    let primary = order
        .iter()
        .find(|name| screens.iter().any(|screen| screen.name == **name))
        .cloned()
        .or_else(|| {
            screens
                .iter()
                .find(|screen| screen.x == 0 && screen.y == 0)
                .map(|screen| screen.name.clone())
        })
        .or_else(|| screens.first().map(|screen| screen.name.clone()));
    if let Some(name) = primary
        && let Some(screen) = screens.iter_mut().find(|screen| screen.name == name)
    {
        screen.primary = true;
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Query {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Query {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        name: &u32,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        let Some(head) = state.heads.get_mut(name) else {
            return;
        };
        match event {
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                let current = flags
                    .into_result()
                    .is_ok_and(|flags| flags.contains(wl_output::Mode::Current));
                if current || head.mode_width == 0 {
                    head.mode_width = width;
                    head.mode_height = height;
                }
            }
            wl_output::Event::Name { name } => head.wl_name = name,
            wl_output::Event::Description { description } => head.wl_description = description,
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for Query {
    fn event(
        _state: &mut Self,
        _proxy: &ZxdgOutputManagerV1,
        _event: <ZxdgOutputManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for Query {
    fn event(
        state: &mut Self,
        _proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        name: &u32,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        let Some(head) = state.heads.get_mut(name) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                head.x = x;
                head.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                head.width = width;
                head.height = height;
            }
            zxdg_output_v1::Event::Name { name } => head.name = name,
            zxdg_output_v1::Event::Description { description } => head.description = description,
            _ => {}
        }
    }
}

impl Dispatch<kde_output_order_v1::KdeOutputOrderV1, ()> for Query {
    fn event(
        state: &mut Self,
        _proxy: &kde_output_order_v1::KdeOutputOrderV1,
        event: kde_output_order_v1::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            kde_output_order_v1::Event::Output { output_name } => {
                if state.order_open {
                    state.order.clear();
                    state.order_open = false;
                }
                state.order.push(output_name);
            }
            kde_output_order_v1::Event::Done => state.order_open = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str, x: i32, primary: bool) -> Screen {
        Screen {
            name: name.into(),
            description: String::new(),
            x,
            y: 0,
            width: 1920,
            height: 1080,
            primary,
        }
    }

    #[test]
    fn a_missing_display_returns_to_primary() {
        let screens = vec![screen("DP-2", 1920, false), screen("eDP-1", 0, true)];
        assert_eq!(
            choose(false, "HDMI-A-1", &screens, None).as_deref(),
            Some("eDP-1")
        );
        assert_eq!(
            choose(false, "DP-2", &screens, None).as_deref(),
            Some("DP-2")
        );
        assert_eq!(choose(false, "", &screens, None).as_deref(), Some("eDP-1"));
    }

    #[test]
    fn follow_cursor_uses_the_active_output_when_it_is_connected() {
        let screens = vec![screen("DP-2", 1920, false), screen("eDP-1", 0, true)];
        assert_eq!(
            choose(true, "eDP-1", &screens, Some("DP-2")).as_deref(),
            Some("DP-2")
        );
        assert_eq!(
            choose(true, "eDP-1", &screens, Some("gone")).as_deref(),
            Some("eDP-1")
        );
    }

    #[test]
    fn busctl_string_keeps_the_output_name() {
        assert_eq!(parse_busctl_string("s \"DP-1\"\n").as_deref(), Some("DP-1"));
        assert_eq!(parse_busctl_string("nope"), None);
    }
}

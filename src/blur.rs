//! KWin background blur for the drawer plate.
//!
//! `org_kde_kwin_blur` has no strength control. The region is the drawer
//! rectangle, not the full-screen surface, so the empty margin stays clear.
//! Other compositors do not advertise the global and the toggle is a no-op
//! there. Blur only shows through pixels that are not fully opaque.
//!
//! KWin copies the region when a blur object is committed onto the surface,
//! and the effect keeps that copy until the object is replaced. `set_region`
//! on an existing object therefore does nothing visible. An empty region is
//! also treated as the whole surface. The drawer never sends one: until its
//! rectangle is known it has no blur at all, and every later rectangle is a
//! new object committed with that rectangle already set.

use std::cell::RefCell;

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager;

struct State;

struct Live {
    surface: WlSurface,
    blur: Option<OrgKdeKwinBlur>,
    /// The centered settings window covers its whole surface. The drawer does not.
    whole: bool,
    /// Rectangle last committed for a drawer surface. Settings leaves this empty.
    applied: Option<(i32, i32, i32, i32)>,
}

struct Host {
    queue: wayland_client::EventQueue<State>,
    qh: QueueHandle<State>,
    compositor: WlCompositor,
    manager: OrgKdeKwinBlurManager,
    live: Vec<Live>,
    enabled: bool,
    drawer: Option<(i32, i32, i32, i32)>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Bind the blur manager on the connection the layer shell will share.
pub fn bind(conn: &Connection, globals: &GlobalList) {
    let queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    let compositor = match globals.bind::<WlCompositor, _, _>(&qh, 1..=6, ()) {
        Ok(compositor) => compositor,
        Err(error) => {
            eprintln!("subtext: no wl_compositor for blur regions: {error}");
            return;
        }
    };
    let manager = match globals.bind::<OrgKdeKwinBlurManager, _, _>(&qh, 1..=1, ()) {
        Ok(manager) => manager,
        Err(_) => {
            eprintln!(
                "subtext: this compositor has no org_kde_kwin_blur_manager, so background blur is unavailable"
            );
            return;
        }
    };
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(Host {
            queue,
            qh,
            compositor,
            manager,
            live: Vec::new(),
            enabled: false,
            drawer: None,
        });
    });
}

/// A new layer surface exists, before its first commit.
pub fn on_surface(surface: &WlSurface, anchor_bits: u32) {
    HOST.with(|slot| {
        let mut host = slot.borrow_mut();
        let Some(host) = host.as_mut() else {
            return;
        };
        prune(host);
        host.live.push(Live {
            surface: surface.clone(),
            blur: None,
            whole: anchor_bits == 0,
            applied: None,
        });
        // The shell commits this surface as soon as the hook returns.
        apply(host, host.live.len() - 1, false);
    });
}

pub fn set_enabled(enabled: bool) {
    HOST.with(|slot| {
        let mut host = slot.borrow_mut();
        let Some(host) = host.as_mut() else {
            return;
        };
        prune(host);
        if host.enabled == enabled {
            return;
        }
        host.enabled = enabled;
        let count = host.live.len();
        for index in 0..count {
            apply(host, index, true);
        }
    });
}

/// Drop the drawer rectangle. Settings keeps the blur on its own window.
///
/// The surface is not committed here. The redraw that follows publishes the
/// detach with the buffer that no longer covers that spot.
pub fn clear_drawer() {
    HOST.with(|slot| {
        let mut host = slot.borrow_mut();
        let Some(host) = host.as_mut() else {
            return;
        };
        prune(host);
        if host.drawer.is_none() {
            return;
        }
        host.drawer = None;
        if !host.enabled {
            return;
        }
        let count = host.live.len();
        for index in 0..count {
            if !host.live[index].whole {
                apply(host, index, false);
            }
        }
    });
}

/// Replace the drawer blur object even when the rectangle did not move.
///
/// KWin keeps the frost it copied until the object is replaced. A chrome
/// change, such as the UI size, commits a new buffer without moving the
/// plate, and the old frost stays wrong until something builds a new object.
/// The redraw that follows publishes it. This does not commit the surface.
pub fn refresh_drawer() {
    HOST.with(|slot| {
        let mut host = slot.borrow_mut();
        let Some(host) = host.as_mut() else {
            return;
        };
        prune(host);
        if !host.enabled || host.drawer.is_none() {
            return;
        }
        let count = host.live.len();
        for index in 0..count {
            if !host.live[index].whole {
                host.live[index].applied = None;
                apply(host, index, false);
            }
        }
    });
}

/// Drawer rectangle in surface coordinates. Settings blurs its whole window.
pub fn set_drawer(origin: (i32, i32), size: (u32, u32)) {
    let region = (origin.0, origin.1, size.0 as i32, size.1 as i32);
    if region.2 <= 0 || region.3 <= 0 {
        return;
    }
    HOST.with(|slot| {
        let mut host = slot.borrow_mut();
        let Some(host) = host.as_mut() else {
            return;
        };
        prune(host);
        if host.drawer == Some(region) {
            return;
        }
        host.drawer = Some(region);
        if !host.enabled {
            return;
        }
        let count = host.live.len();
        for index in 0..count {
            if !host.live[index].whole {
                // The redraw that follows commits the new buffer. Committing
                // here would publish the new region against the previous buffer.
                apply(host, index, false);
            }
        }
    });
}

/// The drawer blur matches the current rectangle, or blur is not in use.
///
/// Callers paint the plate fully opaque until this is true, so the first
/// frame is never a full-screen frost and never a translucent plate with
/// no frost behind it.
pub fn drawer_ready() -> bool {
    HOST.with(|slot| {
        let host = slot.borrow();
        let Some(host) = host.as_ref() else {
            return true;
        };
        if !host.enabled {
            return true;
        }
        let Some(rect) = host.drawer else {
            return false;
        };
        host.live.iter().any(|live| {
            live.surface.is_alive()
                && !live.whole
                && live.blur.is_some()
                && live.applied == Some(rect)
        })
    })
}

fn prune(host: &mut Host) {
    host.live.retain(|live| live.surface.is_alive());
}

fn detach(host: &mut Host, index: usize) {
    let surface = host.live[index].surface.clone();
    if let Some(blur) = host.live[index].blur.take() {
        host.manager.unset(&surface);
        blur.release();
    }
    host.live[index].applied = None;
}

fn apply(host: &mut Host, index: usize, commit_surface: bool) {
    if index >= host.live.len() || !host.live[index].surface.is_alive() {
        return;
    }
    let surface = host.live[index].surface.clone();
    if !host.enabled {
        if host.live[index].blur.is_some() {
            detach(host, index);
            if commit_surface {
                surface.commit();
            }
            let _ = host.queue.flush();
        }
        return;
    }

    if host.live[index].whole {
        if host.live[index].blur.is_some() {
            return;
        }
        let blur = host.manager.create(&surface, &host.qh, ());
        // A null region is the whole surface, which is what settings wants.
        blur.set_region(None);
        blur.commit();
        host.live[index].blur = Some(blur);
        if commit_surface {
            surface.commit();
        }
        let _ = host.queue.flush();
        return;
    }

    let Some(rect) = host.drawer else {
        // No rectangle yet. An empty region would frost the whole output.
        if host.live[index].blur.is_some() {
            detach(host, index);
            if commit_surface {
                surface.commit();
            }
            let _ = host.queue.flush();
        }
        return;
    };
    if host.live[index].applied == Some(rect) && host.live[index].blur.is_some() {
        return;
    }

    // Replace the object with the region already set, then commit the surface
    // once. KWin emits blurChanged only for that commit and reads the region then.
    detach(host, index);
    let blur = host.manager.create(&surface, &host.qh, ());
    let region = host.compositor.create_region(&host.qh, ());
    region.add(rect.0, rect.1, rect.2, rect.3);
    blur.set_region(Some(&region));
    blur.commit();
    region.destroy();
    host.live[index].blur = Some(blur);
    host.live[index].applied = Some(rect);
    if commit_surface {
        surface.commit();
    }
    let _ = host.queue.flush();
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);

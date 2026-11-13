//! Background blur for the drawer plate.
//!
//! Plasma 6.7 and other compositors advertise `ext_background_effect_v1`.
//! Older KWin still advertises `org_kde_kwin_blur`. This binds the first of
//! those that can actually blur. Neither protocol has a strength control, and
//! a compositor that offers neither leaves the toggle as a no-op. Blur only
//! shows through pixels that are not fully opaque.
//!
//! The region is the drawer rectangle, not the full-screen surface, so the
//! empty margin stays clear. An empty region is the whole surface on the old
//! protocol and means "no blur" on the new one, so the drawer never sends one.
//! Until its rectangle is known it has no blur at all.
//!
//! On the old protocol KWin copies the region when the blur object is
//! committed and ignores later `set_region` calls, so a new rectangle is a new
//! object. On `ext_background_effect_v1` the region is double-buffered and
//! applied on the next surface commit, and a second effect object for the
//! same surface is a protocol error. A null region there removes the effect.
//! The settings window therefore sends an explicit region, which the
//! compositor clips to the surface.

use std::cell::{Cell, RefCell};

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_manager_v1::{
    self, Capability, ExtBackgroundEffectManagerV1,
};
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager;

/// Larger than a settings window. The compositor clips it to the surface.
const WHOLE: i32 = 32_768;

struct State;

enum Backend {
    Ext(ExtBackgroundEffectManagerV1),
    Kde(OrgKdeKwinBlurManager),
}

enum Effect {
    Ext(ExtBackgroundEffectSurfaceV1),
    Kde(OrgKdeKwinBlur),
}

struct Live {
    surface: WlSurface,
    effect: Option<Effect>,
    /// The centered settings window covers its whole surface. The drawer does not.
    whole: bool,
    /// Rectangle last requested for a drawer surface. Settings leaves this empty.
    applied: Option<(i32, i32, i32, i32)>,
    /// A blur region is currently requested. The ext object stays after a clear.
    engaged: bool,
}

struct Host {
    queue: wayland_client::EventQueue<State>,
    qh: QueueHandle<State>,
    compositor: WlCompositor,
    backend: Backend,
    live: Vec<Live>,
    enabled: bool,
    drawer: Option<(i32, i32, i32, i32)>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static EXT_BLUR: Cell<bool> = const { Cell::new(false) };
}

/// Bind a blur protocol on the connection the layer shell will share.
///
/// `ext_background_effect_v1` wins when the compositor reports the blur
/// capability. Otherwise the older KWin global is used.
pub fn bind(conn: &Connection, globals: &GlobalList) {
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    let compositor = match globals.bind::<WlCompositor, _, _>(&qh, 1..=6, ()) {
        Ok(compositor) => compositor,
        Err(error) => {
            eprintln!("subtext: no wl_compositor for blur regions: {error}");
            return;
        }
    };
    if let Some(backend) = choose_backend(globals, &mut queue, &qh) {
        HOST.with(|slot| {
            *slot.borrow_mut() = Some(Host {
                queue,
                qh,
                compositor,
                backend,
                live: Vec::new(),
                enabled: false,
                drawer: None,
            });
        });
        return;
    }
    eprintln!(
        "subtext: this compositor has neither ext_background_effect_v1 nor org_kde_kwin_blur_manager, so background blur is unavailable"
    );
}

fn choose_backend(
    globals: &GlobalList,
    queue: &mut wayland_client::EventQueue<State>,
    qh: &QueueHandle<State>,
) -> Option<Backend> {
    if let Ok(manager) = globals.bind::<ExtBackgroundEffectManagerV1, _, _>(qh, 1..=1, ()) {
        EXT_BLUR.set(false);
        if queue.roundtrip(&mut State).is_err() {
            eprintln!("subtext: could not read ext_background_effect_v1 capabilities");
        }
        if EXT_BLUR.get() {
            return Some(Backend::Ext(manager));
        }
        manager.destroy();
        let _ = queue.flush();
    }
    globals
        .bind::<OrgKdeKwinBlurManager, _, _>(qh, 1..=1, ())
        .ok()
        .map(Backend::Kde)
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
            effect: None,
            whole: anchor_bits == 0,
            applied: None,
            engaged: false,
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

/// Ask the compositor for the drawer blur again, even when the rectangle did
/// not move.
///
/// The old protocol keeps the frost it copied until the object is replaced. A
/// chrome change commits a new buffer without moving the plate, so the object
/// has to be built again. The newer protocol just needs the region resent
/// before that commit. This does not commit the surface.
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
            live.surface.is_alive() && !live.whole && live.engaged && live.applied == Some(rect)
        })
    })
}

fn prune(host: &mut Host) {
    host.live.retain(|live| live.surface.is_alive());
}

fn apply(host: &mut Host, index: usize, commit_surface: bool) {
    if index >= host.live.len() || !host.live[index].surface.is_alive() {
        return;
    }
    if !host.enabled {
        clear_effect(host, index, commit_surface);
        return;
    }
    if host.live[index].whole {
        engage_whole(host, index, commit_surface);
        return;
    }
    let Some(rect) = host.drawer else {
        // No rectangle yet. An empty region would frost the whole output on
        // the old protocol, and a null region clears the new one.
        clear_effect(host, index, commit_surface);
        return;
    };
    if host.live[index].engaged && host.live[index].applied == Some(rect) {
        return;
    }
    engage_rect(host, index, rect, commit_surface);
}

fn clear_effect(host: &mut Host, index: usize, commit_surface: bool) {
    if host.live[index].effect.is_none() && !host.live[index].engaged {
        return;
    }
    let surface = host.live[index].surface.clone();
    if matches!(host.backend, Backend::Ext(_)) {
        if host.live[index].engaged {
            if let Some(Effect::Ext(effect)) = &host.live[index].effect {
                // Null removes the effect. Destroying the object would make
                // the next request a second object, which is a protocol error.
                effect.set_blur_region(None);
            }
        }
    } else if let Backend::Kde(manager) = &host.backend {
        if let Some(Effect::Kde(blur)) = host.live[index].effect.take() {
            manager.unset(&surface);
            blur.release();
        }
    }
    host.live[index].applied = None;
    host.live[index].engaged = false;
    if commit_surface {
        surface.commit();
    }
    let _ = host.queue.flush();
}

fn engage_whole(host: &mut Host, index: usize, commit_surface: bool) {
    if host.live[index].engaged && host.live[index].effect.is_some() {
        return;
    }
    let surface = host.live[index].surface.clone();
    if matches!(host.backend, Backend::Ext(_)) {
        let Some(effect) = ext_effect(host, index) else {
            return;
        };
        let region = region(host, 0, 0, WHOLE, WHOLE);
        effect.set_blur_region(Some(&region));
        region.destroy();
    } else {
        let Backend::Kde(manager) = &host.backend else {
            return;
        };
        if host.live[index].effect.is_some() {
            return;
        }
        let blur = manager.create(&surface, &host.qh, ());
        // A null region is the whole surface, which is what settings wants.
        blur.set_region(None);
        blur.commit();
        host.live[index].effect = Some(Effect::Kde(blur));
    }
    host.live[index].engaged = true;
    if commit_surface {
        surface.commit();
    }
    let _ = host.queue.flush();
}

fn engage_rect(host: &mut Host, index: usize, rect: (i32, i32, i32, i32), commit_surface: bool) {
    let surface = host.live[index].surface.clone();
    if matches!(host.backend, Backend::Ext(_)) {
        let Some(effect) = ext_effect(host, index) else {
            return;
        };
        let region = region(host, rect.0, rect.1, rect.2, rect.3);
        effect.set_blur_region(Some(&region));
        region.destroy();
    } else {
        let Backend::Kde(manager) = &host.backend else {
            return;
        };
        // Replace the object with the region already set. KWin reads the
        // region when the blur object is committed, not on set_region.
        if let Some(Effect::Kde(blur)) = host.live[index].effect.take() {
            manager.unset(&surface);
            blur.release();
        }
        let blur = manager.create(&surface, &host.qh, ());
        let region = region(host, rect.0, rect.1, rect.2, rect.3);
        blur.set_region(Some(&region));
        blur.commit();
        region.destroy();
        host.live[index].effect = Some(Effect::Kde(blur));
    }
    host.live[index].applied = Some(rect);
    host.live[index].engaged = true;
    if commit_surface {
        surface.commit();
    }
    let _ = host.queue.flush();
}

fn ext_effect(host: &mut Host, index: usize) -> Option<ExtBackgroundEffectSurfaceV1> {
    if let Some(Effect::Ext(effect)) = &host.live[index].effect {
        return Some(effect.clone());
    }
    let Backend::Ext(manager) = &host.backend else {
        return None;
    };
    let surface = host.live[index].surface.clone();
    let effect = manager.get_background_effect(&surface, &host.qh, ());
    host.live[index].effect = Some(Effect::Ext(effect.clone()));
    Some(effect)
}

fn region(host: &Host, x: i32, y: i32, width: i32, height: i32) -> WlRegion {
    let region = host.compositor.create_region(&host.qh, ());
    region.add(x, y, width, height);
    region
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ExtBackgroundEffectManagerV1,
        event: <ExtBackgroundEffectManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            EXT_BLUR.set(flag_has_blur(flags));
        }
    }
}

fn flag_has_blur(flags: WEnum<Capability>) -> bool {
    match flags {
        WEnum::Value(flags) => flags.contains(Capability::Blur),
        WEnum::Unknown(bits) => bits & Capability::Blur.bits() != 0,
    }
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);
delegate_noop!(State: ignore ExtBackgroundEffectSurfaceV1);

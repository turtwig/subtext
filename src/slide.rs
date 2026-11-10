//! Wayland connection shared with the blur hook.
//!
//! The drawer slides by moving its own rectangle. `org_kde_kwin_slide` is not
//! attached: KWin's slide effect force-blurs the whole fullscreen surface and
//! drops that role without repainting the panel, which leaves a strip on the
//! taskbar. This module keeps the connection alive and installs the blur hook
//! before the first commit.

use std::cell::RefCell;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

struct SlideState;

struct Host {
    _queue: EventQueue<SlideState>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Bind blur on a connection the layer shell will share.
///
/// Connect failure is the only `None`. Compositors without the blur global
/// still get a connection; blur then stays off.
pub fn prepare() -> Option<Connection> {
    let conn = Connection::connect_to_env().ok()?;
    let (globals, queue) = registry_queue_init::<SlideState>(&conn).ok()?;
    let _ = conn.roundtrip();
    crate::blur::bind(&conn, &globals);
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(Host { _queue: queue });
    });
    if let Ok(mut hook) = layershellev::ON_NEW_LAYER_SURFACE.lock() {
        *hook = Some(on_surface);
    }
    Some(conn)
}

fn on_surface(surface: &WlSurface, anchor_bits: u32) {
    crate::blur::on_surface(surface, anchor_bits);
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for SlideState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: <wl_registry::WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

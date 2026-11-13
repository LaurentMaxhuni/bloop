//! Talks to the X server (XWayland on Wayland desktops) for what winit can't do: the notch's
//! window type and taskbar state, its SHAPE outline, monitors and fullscreen detection.

use super::shape;
use iced::window::raw_window_handle::RawWindowHandle;
use std::sync::OnceLock;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// One shared connection; `None` when there is no X server (Wayland-only sessions).
pub(crate) fn connection() -> Option<&'static (RustConnection, usize)> {
    static CONNECTION: OnceLock<Option<(RustConnection, usize)>> = OnceLock::new();
    CONNECTION
        .get_or_init(|| match RustConnection::connect(None) {
            Ok(connection) => Some(connection),
            Err(error) => {
                tracing::warn!("no X11 connection: {error}");
                None
            }
        })
        .as_ref()
}

pub(crate) fn atom(connection: &RustConnection, name: &str) -> Option<xproto::Atom> {
    Some(
        connection
            .intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom,
    )
}

/// The X11 window id behind an Iced window, if it is an X11 window.
pub(crate) fn xid(window: &dyn iced::window::Window) -> Option<xproto::Window> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok(),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    }
}

/// Keeps the notch out of the taskbar, pager and Alt-Tab and above other windows. Runs while
/// the window is still unmapped, because window managers read these when it is first shown.
/// `UTILITY` rather than `DOCK`: Mutter won't give a dock window keyboard focus.
pub fn prepare_notch(window: &dyn iced::window::Window) {
    let (Some((connection, _)), Some(id)) = (connection(), xid(window)) else {
        return;
    };
    let names = [
        "_NET_WM_WINDOW_TYPE",
        "_NET_WM_WINDOW_TYPE_UTILITY",
        "_NET_WM_STATE",
        "_NET_WM_STATE_ABOVE",
        "_NET_WM_STATE_SKIP_TASKBAR",
        "_NET_WM_STATE_SKIP_PAGER",
    ];
    let Some(atoms) = names
        .iter()
        .map(|name| atom(connection, name))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let _ =
        connection.change_property32(PropMode::REPLACE, id, atoms[0], AtomEnum::ATOM, &[atoms[1]]);
    let _ = connection.change_property32(
        PropMode::REPLACE,
        id,
        atoms[2],
        AtomEnum::ATOM,
        &[atoms[3], atoms[4], atoms[5]],
    );
    let _ = connection.flush();
}

/// The scale factor winit uses on X11.
pub fn scale(connection: &RustConnection, screen: usize) -> f32 {
    let env = std::env::var("WINIT_X11_SCALE_FACTOR").ok();
    let resources = connection.setup().roots.get(screen).and_then(|root| {
        let reply = connection
            .get_property(
                false,
                root.root,
                AtomEnum::RESOURCE_MANAGER,
                AtomEnum::STRING,
                0,
                1 << 16,
            )
            .ok()?
            .reply()
            .ok()?;
        String::from_utf8(reply.value).ok()
    });
    shape::scale_factor(env.as_deref(), resources.as_deref())
}

/// Clips the window (drawing and clicks) to the outline, as `SetWindowRgn` does on Windows.
/// Does nothing without the SHAPE extension.
pub fn shape(window: &dyn iced::window::Window, outline: &[(f32, f32)]) {
    use x11rb::protocol::shape::{self as xshape, ConnectionExt as _, SK, SO};
    let (Some((connection, screen)), Some(id)) = (connection(), xid(window)) else {
        return;
    };
    if !matches!(
        connection.extension_information(xshape::X11_EXTENSION_NAME),
        Ok(Some(_))
    ) {
        return;
    }
    let points = shape::scaled_outline(outline, scale(connection, *screen));
    let rects: Vec<xproto::Rectangle> = shape::polygon_rects(&points)
        .into_iter()
        .map(|r| xproto::Rectangle {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        })
        .collect();
    for kind in [SK::BOUNDING, SK::INPUT] {
        let _ = connection.shape_rectangles(
            SO::SET,
            kind,
            xproto::ClipOrdering::UNSORTED,
            id,
            0,
            0,
            &rects,
        );
    }
    let _ = connection.flush();
}

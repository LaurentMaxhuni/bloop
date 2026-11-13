//! Talks to the X server (XWayland on Wayland desktops) for what winit can't do: the notch's
//! window type and taskbar state, its SHAPE outline, monitors and fullscreen detection.

use super::monitors::{self, Monitor};
use super::shape;
use iced::window::raw_window_handle::RawWindowHandle;
use std::sync::OnceLock;
use x11rb::connection::{Connection, RequestConnection};
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

fn root(connection: &RustConnection, screen: usize) -> Option<xproto::Window> {
    connection.setup().roots.get(screen).map(|root| root.root)
}

/// Every active monitor, from RandR.
pub fn monitor_list(connection: &RustConnection, screen: usize) -> Vec<Monitor> {
    use x11rb::protocol::randr::ConnectionExt as _;
    let Some(root) = root(connection, screen) else {
        return Vec::new();
    };
    connection
        .randr_get_monitors(root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| {
            reply
                .monitors
                .iter()
                .map(|m| Monitor {
                    x: m.x.into(),
                    y: m.y.into(),
                    width: m.width.into(),
                    height: m.height.into(),
                    primary: m.primary,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cardinals(
    connection: &RustConnection,
    window: xproto::Window,
    name: &str,
    kind: impl Into<xproto::Atom>,
    count: u32,
) -> Option<Vec<u32>> {
    let property = atom(connection, name)?;
    let reply = connection
        .get_property(false, window, property, kind, 0, count)
        .ok()?
        .reply()
        .ok()?;
    let values: Vec<u32> = reply.value32()?.collect();
    (!values.is_empty()).then_some(values)
}

/// The first desktop's `_NET_WORKAREA` as `[left, top, right, bottom]`.
fn desktop_work_area(connection: &RustConnection, screen: usize) -> Option<[i32; 4]> {
    let values = cardinals(
        connection,
        root(connection, screen)?,
        "_NET_WORKAREA",
        AtomEnum::CARDINAL,
        4,
    )?;
    let [x, y, w, h] = <[u32; 4]>::try_from(values.get(..4)?).ok()?;
    Some([x as i32, y as i32, (x + w) as i32, (y + h) as i32])
}

/// A window's rectangle in root coordinates.
fn root_rect(
    connection: &RustConnection,
    screen: usize,
    window: xproto::Window,
) -> Option<(i32, i32, u32, u32)> {
    let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
    let origin = connection
        .translate_coordinates(window, root(connection, screen)?, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    Some((
        origin.dst_x.into(),
        origin.dst_y.into(),
        geometry.width.into(),
        geometry.height.into(),
    ))
}

fn centre((x, y, width, height): (i32, i32, u32, u32)) -> (i32, i32) {
    (x + width as i32 / 2, y + height as i32 / 2)
}

/// Places the notch on the monitor `pointer` names (else the one it's on, else the primary).
pub fn place(
    window: &dyn iced::window::Window,
    canvas: iced::Size,
    position: bloop::config::NotchPosition,
    use_pointer: bool,
) {
    use x11rb::protocol::xproto::ConfigureWindowAux;
    let (Some((connection, screen)), Some(id)) = (connection(), xid(window)) else {
        return;
    };
    let list = monitor_list(connection, *screen);
    let current = root_rect(connection, *screen, id)
        .and_then(|rect| monitors::monitor_at(&list, centre(rect)));
    let pointer = use_pointer
        .then(|| {
            let reply = connection
                .query_pointer(root(connection, *screen)?)
                .ok()?
                .reply()
                .ok()?;
            Some((i32::from(reply.root_x), i32::from(reply.root_y)))
        })
        .flatten();
    let Some(index) = monitors::pick_monitor(&list, pointer, current) else {
        return;
    };
    let area = monitors::work_area(&list[index], desktop_work_area(connection, *screen));
    let rect = super::super::physical_rect(area, scale(connection, *screen), canvas, position);
    let (width, height) = (rect.width.max(1), rect.height.max(1));
    // The notch isn't resizable, so winit pins min = max = its first size in the size hints,
    // and window managers then refuse the resize below. Pin the new size instead.
    let _ = fixed_size_hints(rect.x, rect.y, width, height).set_normal_hints(connection, id);
    let _ = connection.configure_window(
        id,
        &ConfigureWindowAux::new()
            .x(rect.x)
            .y(rect.y)
            .width(width as u32)
            .height(height as u32),
    );
    let _ = connection.flush();
}

/// Size hints that fix the window at `width` x `height`, placed at `(x, y)` by the program.
fn fixed_size_hints(x: i32, y: i32, width: i32, height: i32) -> x11rb::properties::WmSizeHints {
    use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification::ProgramSpecified};
    let mut hints = WmSizeHints::new();
    hints.position = Some((ProgramSpecified, x, y));
    hints.size = Some((ProgramSpecified, width, height));
    hints.min_size = Some((width, height));
    hints.max_size = Some((width, height));
    hints
}

/// The primary monitor's work area in logical units, and the scale.
pub fn primary_area() -> Option<([f32; 4], f32)> {
    let (connection, screen) = connection()?;
    let list = monitor_list(connection, *screen);
    let index = monitors::pick_monitor(&list, None, None)?;
    let area = monitors::work_area(&list[index], desktop_work_area(connection, *screen));
    let scale = scale(connection, *screen);
    Some((area.map(|value| value as f32 / scale), scale))
}

/// True when the active window is fullscreen and covers its monitor. On Wayland sessions
/// only X11 apps are visible here. Any error counts as "not fullscreen".
pub fn fullscreen_active() -> bool {
    let Some((connection, screen)) = connection() else {
        return false;
    };
    let check = || -> Option<bool> {
        let root = root(connection, *screen)?;
        let active =
            *cardinals(connection, root, "_NET_ACTIVE_WINDOW", AtomEnum::WINDOW, 1)?.first()?;
        if active == 0 {
            return Some(false);
        }
        let fullscreen = atom(connection, "_NET_WM_STATE_FULLSCREEN")?;
        let states =
            cardinals(connection, active, "_NET_WM_STATE", AtomEnum::ATOM, 32).unwrap_or_default();
        if !states.contains(&fullscreen) {
            return Some(false);
        }
        let rect = root_rect(connection, *screen, active)?;
        let list = monitor_list(connection, *screen);
        let index = monitors::monitor_at(&list, centre(rect))?;
        Some(monitors::covers_monitor(rect, &list[index]))
    };
    check().unwrap_or(false)
}

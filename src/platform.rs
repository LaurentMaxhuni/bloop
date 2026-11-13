// TEMPORARY (Linux port in progress): removed once every Linux function is wired up.
#![cfg_attr(target_os = "linux", allow(dead_code))]

#[cfg(any(target_os = "linux", test))]
pub mod linux;

use bloop::config::NotchPosition;
use iced::{Point, Size, futures::SinkExt};

pub fn show_fatal(message: &str) {
    tracing::error!("Startup failed: {message}");
    #[cfg(windows)]
    {
        use windows::{
            Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
            core::PCWSTR,
        };
        let message: Vec<u16> = format!(
            "bloop could not start. A DirectX 12-capable graphics driver is required.\n\n{message}"
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let title: Vec<u16> = "bloop — startup error"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: UTF-16 strings remain alive and NUL-terminated throughout the modal call.
        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(target_os = "linux")]
    linux::portal::show_fatal(message);
    #[cfg(not(any(windows, target_os = "linux")))]
    eprintln!("bloop could not start: {message}");
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Controller),
    Toggle,
    Ask,
    Settings,
    Quit,
    Warning(String),
    ShortcutApplied,
    /// Linux Wayland: the desktop manages the shortcuts; `(id, key combination)` as reported.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    ShortcutsManagedBySystem(Vec<(String, String)>),
    /// A lasting note about this desktop (e.g. no tray), shown on the Shortcut & startup page.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Note(String),
}

#[derive(Debug, Clone)]
pub struct Controller {
    #[cfg(windows)]
    sender: std::sync::mpsc::Sender<Command>,
    #[cfg(windows)]
    thread_id: u32,
    #[cfg(target_os = "linux")]
    linux: tokio::sync::mpsc::UnboundedSender<linux::shortcuts::Command>,
}

#[cfg(windows)]
#[derive(Default)]
struct ThreadState {
    stopping: std::sync::atomic::AtomicBool,
    id: std::sync::atomic::AtomicU32,
}

#[cfg(windows)]
enum Command {
    Shortcut(String),
    AskShortcut(String),
}

impl Controller {
    /// Empty disables the clipboard shortcut. The UI wires this up separately.
    #[allow(dead_code)]
    pub fn ask_shortcut(&self, value: String) {
        #[cfg(windows)]
        self.send(Command::AskShortcut(value));
        #[cfg(target_os = "linux")]
        let _ = self
            .linux
            .send(linux::shortcuts::Command::AskShortcut(value));
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = value;
    }

    pub fn shortcut(&self, value: String) {
        #[cfg(windows)]
        self.send(Command::Shortcut(value));
        #[cfg(target_os = "linux")]
        let _ = self.linux.send(linux::shortcuts::Command::Shortcut(value));
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = value;
    }

    #[cfg(windows)]
    fn send(&self, command: Command) {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::PostThreadMessageW,
        };
        if self.sender.send(command).is_ok() {
            // SAFETY: ID belongs to our thread; its message queue exists before Ready is sent.
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WAKE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn validate_shortcut(value: &str) -> Result<(), String> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        value
            .parse::<global_hotkey::hotkey::HotKey>()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = value;
        Ok(())
    }
}

/// Show a native Markdown Save dialog. Call from a blocking worker, not Iced's
/// event loop. No borrowed HWND or COM apartment is needed; all buffers are local.
/// Only selects a path: the caller is responsible for writing the transcript.
#[allow(dead_code)] // UI integration is owned by the companion branch.
pub fn save_file_dialog(suggested_name: &str) -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows::{
            Win32::UI::Controls::Dialogs::{
                CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR,
                OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
            },
            core::{PCWSTR, PWSTR, w},
        };
        let suggested = wide(&bloop::export::file_name(suggested_name));
        let mut path = vec![0u16; 32_768];
        path[..suggested.len()].copy_from_slice(&suggested);
        let filter: Vec<u16> = "Markdown (*.md)\0*.md\0\0".encode_utf16().collect();
        let mut dialog = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            nFilterIndex: 1,
            lpstrFile: PWSTR(path.as_mut_ptr()),
            nMaxFile: path.len() as u32,
            lpstrTitle: w!("Export chat as Markdown"),
            lpstrDefExt: w!("md"),
            Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
            ..Default::default()
        };
        // SAFETY: dialog's size is correct; writable path and double-NUL filter
        // buffers stay alive for this synchronous call, with no hooks or owner HWND.
        if unsafe { GetSaveFileNameW(&mut dialog).as_bool() } {
            let length = path
                .iter()
                .position(|c| *c == 0)
                .ok_or("Windows returned an unterminated save path.")?;
            return Ok(Some(std::ffi::OsString::from_wide(&path[..length]).into()));
        }
        // SAFETY: reads this calling thread's dialog error immediately after failure.
        let error = unsafe { CommDlgExtendedError() }.0;
        if error == 0 {
            Ok(None)
        } else {
            Err(format!(
                "Could not open the Save dialog (Windows dialog error 0x{error:04X})."
            ))
        }
    }
    #[cfg(target_os = "linux")]
    {
        linux::portal::save_file_dialog(suggested_name)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = suggested_name;
        Err("The Save dialog is only available on Windows.".into())
    }
}

pub fn events() -> impl iced::futures::Stream<Item = Event> {
    iced::stream::channel(16, async |mut output| {
        #[cfg(windows)]
        {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let (commands, inbox) = std::sync::mpsc::channel();
            let worker_sender = sender.clone();
            let state = std::sync::Arc::new(ThreadState::default());
            let worker_state = state.clone();
            let thread = std::thread::spawn(move || {
                if let Err(error) = run(worker_sender.clone(), commands, inbox, worker_state) {
                    let _ = worker_sender.send(Event::Warning(error));
                }
            });
            // Receiver owns thread lifetime; aborting subscription requests shutdown.
            struct Guard {
                state: std::sync::Arc<ThreadState>,
                thread: Option<std::thread::JoinHandle<()>>,
            }
            impl Drop for Guard {
                fn drop(&mut self) {
                    use std::sync::atomic::Ordering;
                    self.state.stopping.store(true, Ordering::Release);
                    let id = self.state.id.load(Ordering::Acquire);
                    if id != 0 {
                        use windows::Win32::{
                            Foundation::{LPARAM, WPARAM},
                            UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
                        };
                        // SAFETY: ID is published only after our worker creates its message queue.
                        unsafe {
                            let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
                        }
                    }
                    if let Some(thread) = self.thread.take() {
                        let _ = thread.join();
                    }
                }
            }
            let _guard = Guard {
                state,
                thread: Some(thread),
            };
            drop(sender);
            while let Some(event) = receiver.recv().await {
                if output.send(event).await.is_err() {
                    break;
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
            let _ = sender.send(Event::Ready(Controller { linux: commands }));
            if linux::session() == linux::Session::WaylandOnly {
                let _ = sender.send(Event::Note(
                    "Install XWayland for the notch to stay on top.".into(),
                ));
            }
            // Stop both when the subscription ends.
            struct Abort(Vec<tokio::task::JoinHandle<()>>);
            impl Drop for Abort {
                fn drop(&mut self) {
                    self.0.iter().for_each(tokio::task::JoinHandle::abort);
                }
            }
            let _tasks = Abort(vec![
                tokio::spawn(linux::portal::watch_settings()),
                tokio::spawn(linux::tray::run(sender.clone())),
                tokio::spawn(linux::shortcuts::run(sender, inbox)),
            ]);
            while let Some(event) = receiver.recv().await {
                if output.send(event).await.is_err() {
                    break;
                }
            }
            std::future::pending::<()>().await;
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = &mut output;
            std::future::pending::<()>().await;
        }
    })
}

#[cfg(windows)]
const WAKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 42;

#[cfg(any(windows, target_os = "linux"))]
#[derive(Default)]
pub(crate) struct Shortcuts([Option<global_hotkey::hotkey::HotKey>; 2]);

#[cfg(any(windows, target_os = "linux"))]
impl Shortcuts {
    pub(crate) fn event(&self, id: u32) -> Option<Event> {
        if self.0[0].is_some_and(|key| key.id() == id) {
            Some(Event::Toggle)
        } else if self.0[1].is_some_and(|key| key.id() == id) {
            Some(Event::Ask)
        } else {
            None
        }
    }

    pub(crate) fn apply(
        &mut self,
        slot: usize,
        value: &str,
        mut register: impl FnMut(bool, global_hotkey::hotkey::HotKey) -> Result<(), String>,
    ) -> Result<(), String> {
        let next = if slot == 1 && value.is_empty() {
            None
        } else {
            validate_shortcut(value)?;
            Some(
                value
                    .parse::<global_hotkey::hotkey::HotKey>()
                    .map_err(|e| e.to_string())?,
            )
        };
        if next == self.0[slot] {
            return Ok(());
        }
        if next.is_some() && next == self.0[1 - slot] {
            return Err("This key combination is already used by the other bloop shortcut.".into());
        }
        if let Some(key) = next {
            register(true, key)?;
        }
        if let Some(old) = self.0[slot]
            && let Err(error) = register(false, old)
        {
            if let Some(key) = next {
                let _ = register(false, key);
            }
            return Err(error);
        }
        self.0[slot] = next;
        Ok(())
    }
}

#[cfg(windows)]
fn run(
    sender: tokio::sync::mpsc::UnboundedSender<Event>,
    commands: std::sync::mpsc::Sender<Command>,
    inbox: std::sync::mpsc::Receiver<Command>,
    state: std::sync::Arc<ThreadState>,
) -> Result<(), String> {
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use tray_icon::{
        MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuItem},
    };
    use windows::Win32::{
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, TranslateMessage,
        },
    };

    let mut msg = MSG::default();
    // SAFETY: valid stack MSG; PeekMessage creates this worker's message queue.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    // SAFETY: returns the calling thread's ID without taking pointers.
    let thread_id = unsafe { GetCurrentThreadId() };
    state
        .id
        .store(thread_id, std::sync::atomic::Ordering::Release);
    if state.stopping.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(());
    }
    let controller = Controller {
        sender: commands,
        thread_id,
    };
    sender
        .send(Event::Ready(controller))
        .map_err(|e| e.to_string())?;
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let hotkey_sender = sender.clone();
    let shortcuts = std::sync::Arc::new(std::sync::Mutex::new(Shortcuts::default()));
    let hotkey_shortcuts = shortcuts.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed
            && let Some(action) = hotkey_shortcuts.lock().unwrap().event(event.id)
        {
            let _ = hotkey_sender.send(action);
        }
    }));
    let menu = Menu::new();
    let toggle = MenuItem::new("Show / collapse bloop", true, None);
    let settings = MenuItem::new("Settings", true, None);
    let quit = MenuItem::new("Quit bloop", true, None);
    menu.append_items(&[&toggle, &settings, &quit])
        .map_err(|e| e.to_string())?;
    let toggle_id = toggle.id().clone();
    let settings_id = settings.id().clone();
    let quit_id = quit.id().clone();
    let menu_sender = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = if event.id == toggle_id {
            Event::Toggle
        } else if event.id == settings_id {
            Event::Settings
        } else if event.id == quit_id {
            Event::Quit
        } else {
            return;
        };
        let _ = menu_sender.send(action);
    }));
    let tray_sender = sender.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = tray_sender.send(Event::Toggle);
        }
    }));
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            let inside = (x - 16).pow(2) + (y - 16).pow(2) < 14 * 14;
            let eye = (x == 12 || x == 20) && (12..=18).contains(&y);
            rgba.extend_from_slice(if eye {
                &[20, 25, 23, 255]
            } else if inside {
                &[190, 239, 159, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    let icon = tray_icon::Icon::from_rgba(rgba, 32, 32).map_err(|e| e.to_string())?;
    let _tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("bloop · your local AI notch")
        .build()
        .map_err(|e| e.to_string())?;
    loop {
        if state.stopping.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        // SAFETY: valid MSG storage, no HWND filter; blocks without CPU polling.
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
        if result == -1 {
            return Err("Windows desktop message loop failed.".into());
        }
        if result == 0 {
            break;
        }
        if msg.message == WAKE {
            for command in inbox.try_iter() {
                let (slot, value, label) = match command {
                    Command::Shortcut(value) => (0, value, "Global shortcut"),
                    Command::AskShortcut(value) => (1, value, "Ask about clipboard shortcut"),
                };
                let result = shortcuts.lock().unwrap().apply(slot, &value, |add, key| {
                    if add {
                        manager.register(key)
                    } else {
                        manager.unregister(key)
                    }
                    .map_err(|e| e.to_string())
                });
                let _ = sender.send(match result {
                    Ok(()) => Event::ShortcutApplied,
                    Err(error) => Event::Warning(format!("{label} unavailable: {error}")),
                });
            }
        } else {
            // SAFETY: MSG was initialized by a successful GetMessageW on this thread.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    Ok(())
}

/// Clips the window to `outline` (logical units). wgpu's DX12 HWND swapchain is
/// opaque, so a window region is what makes the area outside the notch see-through.
pub fn shape(window: &dyn iced::window::Window, outline: &[(f32, f32)]) {
    #[cfg(windows)]
    {
        use iced::window::raw_window_handle::RawWindowHandle;
        use windows::Win32::{
            Foundation::{HWND, POINT},
            Graphics::Gdi::{CreatePolygonRgn, DeleteObject, SetWindowRgn, WINDING},
            UI::HiDpi::GetDpiForWindow,
        };
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        // SAFETY: hwnd comes from the live window that owns this callback.
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        let points: Vec<POINT> = outline
            .iter()
            .map(|&(x, y)| POINT {
                x: (x * scale).round() as i32,
                y: (y * scale).round() as i32,
            })
            .collect();
        // SAFETY: the slice is a valid, initialized POINT array for the call's duration.
        let region = unsafe { CreatePolygonRgn(&points, WINDING) };
        if region.is_invalid() {
            return;
        }
        // SAFETY: on success the system takes ownership of the region; on failure we free it.
        if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
            // SAFETY: the region was not adopted by the window, so it is still ours.
            let _ = unsafe { DeleteObject(region.into()) };
        }
    }
    #[cfg(target_os = "linux")]
    linux::x11::shape(window, outline);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (window, outline);
    }
}

/// Linux: marks the still-hidden notch as a utility window kept out of the taskbar and pager.
/// Other platforms set this up through the window settings.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn prepare_notch(window: &dyn iced::window::Window) {
    #[cfg(target_os = "linux")]
    linux::x11::prepare_notch(window);
    #[cfg(not(target_os = "linux"))]
    let _ = window;
}

/// Opens a link from a reply in the default browser. Only web and mail links: a reply
/// must not be able to launch files, programs or other URI handlers.
pub fn open_url(url: &str) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    if !["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        || url.chars().any(char::is_control)
    {
        return Err(format!("bloop only opens web links: {url}"));
    }
    #[cfg(windows)]
    {
        use windows::{
            Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
            core::{HSTRING, w},
        };
        let target = HSTRING::from(url.trim());
        // SAFETY: every string is a live, NUL-terminated wide string for the whole call.
        let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
        // ShellExecute reports success as a value greater than 32.
        if result.0 as usize <= 32 {
            return Err("Couldn't open the link.".into());
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        linux::portal::open_url(url.trim())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err("Opening links isn't supported on this platform yet.".into())
    }
}

/// True while a fullscreen app, presentation, or the lock screen should hide the notch.
/// Errors count as "not fullscreen" so the notch is never stranded.
pub fn fullscreen_active() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
            SHQueryUserNotificationState,
        };
        // SAFETY: takes no pointers from us; the wrapper owns its out-parameter.
        unsafe { SHQueryUserNotificationState() }.is_ok_and(|state| {
            [
                QUNS_BUSY,
                QUNS_NOT_PRESENT,
                QUNS_PRESENTATION_MODE,
                QUNS_RUNNING_D3D_FULL_SCREEN,
            ]
            .contains(&state)
        })
    }
    #[cfg(target_os = "linux")]
    {
        linux::x11::fullscreen_active()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// Primary monitor work area; physical coordinates converted to logical units.
/// Gap between a left/right notch and the edge of the screen.
pub const EDGE_INSET: f32 = 24.0;

#[derive(Debug, Clone, Copy)]
pub enum Target {
    Current,
    Active,
}

#[cfg(any(windows, target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
struct PhysicalRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[cfg(any(windows, target_os = "linux", test))]
fn physical_rect(
    area: [i32; 4],
    scale: f32,
    canvas: Size,
    position: NotchPosition,
) -> PhysicalRect {
    let [left, top, right, _bottom] = area;
    let width = (canvas.width * scale).round() as i32;
    let height = (canvas.height * scale).round() as i32;
    let inset = (EDGE_INSET * scale).round() as i32;
    let x = match position {
        NotchPosition::Centre => left + (right - left - width) / 2,
        NotchPosition::Left => left + inset,
        NotchPosition::Right => right - inset - width,
    };
    PhysicalRect {
        x,
        y: top,
        width,
        height,
    }
}

#[cfg(windows)]
fn target_monitor(
    hwnd: windows::Win32::Foundation::HWND,
    target: Target,
) -> windows::Win32::Graphics::Gdi::HMONITOR {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{
            MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId},
    };
    // SAFETY: live callback HWND; other handles are queried from Windows. All output
    // pointers refer to initialized stack storage. No handle ownership is transferred.
    unsafe {
        if matches!(target, Target::Current) {
            return MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        }
        let foreground = GetForegroundWindow();
        let mut process = 0;
        if !foreground.is_invalid()
            && GetWindowThreadProcessId(foreground, Some(&mut process)) != 0
            && process != std::process::id()
        {
            let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONULL);
            if !monitor.is_invalid() {
                return monitor;
            }
        }
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_ok() {
            return MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        }
        MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY)
    }
}

/// Place the undecorated notch in physical desktop pixels, on the window thread.
pub fn place_on(
    window: &dyn iced::window::Window,
    canvas: Size,
    position: NotchPosition,
    target: Target,
) {
    #[cfg(windows)]
    {
        use iced::window::raw_window_handle::RawWindowHandle;
        use windows::Win32::{
            Foundation::HWND,
            Graphics::Gdi::{GetMonitorInfoW, MONITORINFO},
            UI::{
                HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
                WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
            },
        };
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let monitor = target_monitor(hwnd, target);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: monitor is selected by Windows; cbSize describes valid writable storage.
        if !unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
            return;
        }
        let (mut xdpi, mut ydpi) = (96, 96);
        // SAFETY: valid monitor and output pointers; initialized DPI is retained on failure.
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut xdpi, &mut ydpi) };
        let area = info.rcWork;
        let rect = physical_rect(
            [area.left, area.top, area.right, area.bottom],
            xdpi.max(96) as f32 / 96.0,
            canvas,
            position,
        );
        // The first synchronous move lets winit handle WM_DPICHANGED (including its
        // suggested-rectangle resize). Reapply our exact rectangle after it returns.
        // No ASYNCWINDOWPOS: window::run executes on the owning event-loop thread.
        for _ in 0..2 {
            // SAFETY: HWND is borrowed from this live window callback. Pixel dimensions
            // are computed above; flags preserve activation and the existing Z order.
            if let Err(error) = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            } {
                tracing::warn!("Could not place notch: {error}");
                break;
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // The pointer is only fresh under XWayland while it is over an X11 window, so Wayland
        // sessions keep the notch on its current monitor.
        let use_pointer =
            matches!(target, Target::Active) && linux::session() == linux::Session::X11;
        linux::x11::place(window, canvas, position, use_pointer);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = (window, canvas, position, target);
}

/// Centred top-of-screen origin for a window of `size` on the primary monitor.
pub fn primary_position(size: Size) -> Option<Point> {
    place(size, NotchPosition::Centre)
}

/// Origin for bloop's window of `size` along the top of the primary monitor's work area,
/// in logical units.
pub fn place(size: Size, position: NotchPosition) -> Option<Point> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::POINT,
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
            },
            UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        };
        // SAFETY: no pointers; selects the primary monitor through the fallback flag.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: cbSize matches the valid writable MONITORINFO allocation.
        if !unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
            return None;
        }
        let (mut xdpi, mut ydpi) = (96, 96);
        // SAFETY: valid monitor and output pointers; fallback remains 96 DPI on failure.
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut xdpi, &mut ydpi) };
        let scale = xdpi.max(96) as f32 / 96.0;
        let area = info.rcWork;
        let (left, right) = (area.left as f32 / scale, area.right as f32 / scale);
        let x = match position {
            NotchPosition::Centre => (left + right) / 2.0 - size.width / 2.0,
            NotchPosition::Left => left + EDGE_INSET,
            NotchPosition::Right => right - EDGE_INSET - size.width,
        };
        // Flush with the top edge so it reads as a notch.
        Some(Point::new(x, area.top as f32 / scale))
    }
    #[cfg(target_os = "linux")]
    {
        let ([left, top, right, _bottom], _) = linux::x11::primary_area()?;
        let x = match position {
            NotchPosition::Centre => (left + right) / 2.0 - size.width / 2.0,
            NotchPosition::Left => left + EDGE_INSET,
            NotchPosition::Right => right - EDGE_INSET - size.width,
        };
        Some(Point::new(x, top))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (size, position);
        None
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Holds the app-wide mutex the installer checks (`AppMutex=bloop-app`) and the
/// auto-reset event a second launch signals to ask this copy to open its window.
pub struct Instance {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    open_event: Option<windows::Win32::Foundation::HANDLE>,
    /// The socket's folder, when this copy owns the instance socket.
    #[cfg(target_os = "linux")]
    socket_dir: Option<std::path::PathBuf>,
}

/// The instance socket's listener, handed to [`open_requests`].
#[cfg(target_os = "linux")]
static LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> =
    std::sync::Mutex::new(None);

#[cfg(target_os = "linux")]
fn linux_socket_dir() -> std::path::PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    linux::instance::socket_dir(runtime.as_deref(), uid)
}

impl Drop for Instance {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(dir) = &self.socket_dir {
            linux::instance::remove(dir);
        }
        #[cfg(windows)]
        // SAFETY: both handles are ours (CreateMutexW / CreateEventW) and closed exactly once here.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
            if let Some(event) = self.open_event {
                let _ = windows::Win32::Foundation::CloseHandle(event);
            }
        }
    }
}

#[cfg(windows)]
const MUTEX_NAME: &str = r"Local\bloop-app";
#[cfg(windows)]
const OPEN_EVENT_NAME: &str = r"Local\bloop-open";

/// `None` when another bloop is already running in this session.
pub fn single_instance() -> Option<Instance> {
    #[cfg(windows)]
    {
        instance_named(MUTEX_NAME, OPEN_EVENT_NAME)
    }
    #[cfg(target_os = "linux")]
    {
        let dir = linux_socket_dir();
        match linux::instance::bind(&dir) {
            Ok(Some(listener)) => {
                if let Ok(mut slot) = LISTENER.lock() {
                    *slot = Some(listener);
                }
                Some(Instance {
                    socket_dir: Some(dir),
                })
            }
            Ok(None) => None,
            Err(error) => {
                // Run anyway: a broken socket must not stop bloop from starting.
                tracing::warn!("single-instance socket unavailable: {error}");
                Some(Instance { socket_dir: None })
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Some(Instance {})
    }
}

/// Passes this launch's request to the running bloop (best effort). Windows always opens the
/// app window, as before.
pub fn signal_running_copy(request: bloop::launch::Request) {
    #[cfg(target_os = "linux")]
    if let Err(error) = linux::instance::send(&linux_socket_dir(), request) {
        tracing::warn!("could not reach the running copy: {error}");
    }
    #[cfg(not(target_os = "linux"))]
    let _ = request;
    #[cfg(windows)]
    {
        use windows::Win32::{
            System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent},
            UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
        };
        let name = wide(OPEN_EVENT_NAME);
        // SAFETY: the name is NUL-terminated UTF-16 that outlives the call.
        let event = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE,
                false,
                windows::core::PCWSTR(name.as_ptr()),
            )
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!("could not open the running copy's event: {error}");
                return;
            }
        };
        // SAFETY: lets the running copy raise its window over whatever this launch had focus.
        if let Err(error) = unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
            tracing::warn!("AllowSetForegroundWindow failed: {error}");
        }
        // SAFETY: event is a live handle opened above.
        if let Err(error) = unsafe { SetEvent(event) } {
            tracing::warn!("could not signal the running copy: {error}");
        }
        // SAFETY: we own this handle and do not use it again.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(event);
        }
    }
}

/// Yields `()` each time a second launch asks this copy to open its window.
pub fn open_requests() -> impl iced::futures::Stream<Item = bloop::launch::Request> {
    iced::stream::channel(4, async |mut output| {
        #[cfg(windows)]
        {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            std::thread::spawn(move || wait_for_open_requests(sender));
            while receiver.recv().await.is_some() {
                if output.send(bloop::launch::Request::Open).await.is_err() {
                    break;
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            let listener = LISTENER.lock().ok().and_then(|mut slot| slot.take());
            if let Some(listener) = listener {
                let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
                std::thread::spawn(move || {
                    for stream in listener.incoming().flatten() {
                        if let Some(request) = linux::instance::receive(stream)
                            && sender.send(request).is_err()
                        {
                            break;
                        }
                    }
                });
                while let Some(request) = receiver.recv().await {
                    if output.send(request).await.is_err() {
                        break;
                    }
                }
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = &mut output;
        }
        iced::futures::future::pending::<()>().await;
    })
}

#[cfg(windows)]
fn wait_for_open_requests(sender: tokio::sync::mpsc::UnboundedSender<()>) {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{
            INFINITE, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
        },
    };
    let name = wide(OPEN_EVENT_NAME);
    // SAFETY: the name is NUL-terminated UTF-16 that outlives the call.
    let event = unsafe {
        OpenEventW(
            SYNCHRONIZATION_SYNCHRONIZE,
            false,
            windows::core::PCWSTR(name.as_ptr()),
        )
    };
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!("could not listen for open requests: {error}");
            return;
        }
    };
    // SAFETY: event is a live handle owned by this thread until closed below.
    while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
        tracing::debug!("open request received");
        if sender.send(()).is_err() {
            break;
        }
    }
    // SAFETY: we own this handle and do not use it again.
    unsafe {
        let _ = CloseHandle(event);
    }
}

/// Sets the window's big and small icons from the exe's embedded resource (id 1).
pub fn apply_app_icon(window: &dyn iced::window::Window) {
    #[cfg(windows)]
    {
        use iced::window::raw_window_handle::RawWindowHandle;
        use windows::Win32::{
            Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LR_SHARED,
                LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
                WM_SETICON,
            },
        };
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        // SAFETY: a null module name returns this exe's handle.
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
            return;
        };
        let instance = HINSTANCE(module.0);
        for (kind, width, height) in [
            (ICON_BIG, SM_CXICON, SM_CYICON),
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
        ] {
            // SAFETY: plain metric queries.
            let (width, height) = unsafe { (GetSystemMetrics(width), GetSystemMetrics(height)) };
            // SAFETY: resource id 1 is passed as MAKEINTRESOURCE (an integer in the pointer).
            // LR_SHARED: the system owns and caches the handle, so reopening the window
            // reuses it instead of leaking a new one each time.
            let icon = unsafe {
                LoadImageW(
                    Some(instance),
                    windows::core::PCWSTR(std::ptr::without_provenance::<u16>(1)),
                    IMAGE_ICON,
                    width,
                    height,
                    LR_DEFAULTCOLOR | LR_SHARED,
                )
            };
            if let Ok(icon) = icon {
                // SAFETY: hwnd belongs to the live window; the icon handle stays valid for the process.
                unsafe {
                    SendMessageW(
                        hwnd,
                        WM_SETICON,
                        Some(WPARAM(kind as usize)),
                        Some(LPARAM(icon.0 as isize)),
                    );
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

#[cfg(windows)]
fn instance_named(name: &str, event_name: &str) -> Option<Instance> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::{CreateEventW, CreateMutexW},
        },
        core::PCWSTR,
    };
    let name = wide(name);
    // SAFETY: the name is NUL-terminated UTF-16 that outlives the call; no attributes.
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.ok()?;
    // SAFETY: reads this thread's last error right after CreateMutexW.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: we own this handle and do not use it again.
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    let event_name = wide(event_name);
    // SAFETY: auto-reset, initially unsignalled; the name is NUL-terminated and outlives the call.
    let open_event = unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) }
        .inspect_err(|error| tracing::warn!("could not create the open event: {error}"))
        .ok();
    Some(Instance { handle, open_event })
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE: &str = "bloop";

/// Whether bloop starts when the user signs in (the HKCU Run value exists).
pub fn autostart() -> bool {
    #[cfg(windows)]
    {
        run_value(RUN_KEY).is_some()
    }
    #[cfg(target_os = "linux")]
    {
        linux::desktop::autostart()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

pub fn set_autostart(enable: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        set_run_value(RUN_KEY, enable)
    }
    #[cfg(target_os = "linux")]
    {
        linux::desktop::set_autostart(enable)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = enable;
        Err("Start with Windows is only available on Windows.".into())
    }
}

/// The Run-key command: autostart launches quietly, without opening the app window.
fn run_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --background", exe.display())
}

/// Whether an existing Run value must be rewritten: only this exe's own pre-`--background`
/// entry (`"<exe>"`). Absent means autostart is off; a value naming another exe belongs to
/// another copy (the installed one, when this is a dev build) and is left alone, or every
/// copy launched once would take over sign-in and the installed copy would stop updating.
fn needs_migration(current: Option<&str>, exe: &std::path::Path) -> bool {
    let legacy = format!("\"{}\"", exe.display());
    current.is_some_and(|value| value.trim().eq_ignore_ascii_case(&legacy))
}

/// Rewrites a Run value left by an older build (no `--background`) so sign-in starts quietly.
pub fn migrate_autostart() {
    #[cfg(windows)]
    if let Err(error) = migrate_run_value(RUN_KEY) {
        tracing::warn!("could not migrate the autostart entry: {error}");
    }
    #[cfg(target_os = "linux")]
    {
        linux::desktop::migrate_autostart();
        linux::desktop::ensure_launcher_entry();
    }
}

#[cfg(windows)]
fn migrate_run_value(key: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if needs_migration(run_value(key).as_deref(), &exe) {
        set_run_value(key, true)?;
    }
    Ok(())
}

#[cfg(windows)]
fn run_value(key: &str) -> Option<String> {
    use windows::{
        Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
        core::PCWSTR,
    };
    let (key, name) = (wide(key), wide(RUN_VALUE));
    let mut buffer = vec![0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: strings are NUL-terminated; buffer and its byte size describe one live allocation.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

#[cfg(windows)]
fn set_run_value(key: &str, enable: bool) -> Result<(), String> {
    use windows::{
        Win32::{
            Foundation::ERROR_FILE_NOT_FOUND,
            System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW},
        },
        core::PCWSTR,
    };
    let (key, name) = (wide(key), wide(RUN_VALUE));
    let status = if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let data = wide(&run_command(&exe));
        // SAFETY: strings are NUL-terminated; data pointer and byte length cover the same Vec.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        }
    } else {
        // SAFETY: strings are NUL-terminated and outlive the call.
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status
    };
    status
        .ok()
        .map_err(|e| format!("Could not change start with Windows: {}", e.message()))
}

#[cfg(all(windows, test))]
fn delete_tree(key: &str) {
    use windows::{
        Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW},
        core::PCWSTR,
    };
    let key = wide(key);
    // SAFETY: NUL-terminated key under HKCU used only by tests.
    let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
}

/// True when Windows' "Animation effects" setting is off. Errors count as "animations on".
pub fn reduce_motion_system() -> bool {
    #[cfg(windows)]
    {
        use windows::{
            Win32::UI::WindowsAndMessaging::{
                SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
                SystemParametersInfoW,
            },
            core::BOOL,
        };
        let mut enabled = BOOL(1);
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into the pointer we pass.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&raw mut enabled).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        ok.is_ok() && !enabled.as_bool()
    }
    #[cfg(target_os = "linux")]
    {
        linux::portal::reduce_motion_system()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// True when Windows apps use the light theme. Errors count as dark.
pub fn light_theme_system() -> bool {
    #[cfg(windows)]
    {
        use windows::{
            Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
            core::w,
        };
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: static NUL-terminated strings; `value`/`size` describe one live u32.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&mut size),
            )
        };
        status.is_ok() && value == 1
    }
    #[cfg(target_os = "linux")]
    {
        linux::portal::light_theme_system()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// Starts a downloaded installer silently; it waits for bloop to quit, installs, and opens
/// bloop again (`/RELAUNCH=1`). Previously chosen setup options are kept.
///
/// Setup must outlive bloop. If bloop runs inside a job that kills its processes when the job
/// closes (some launchers and terminals do this), setup is started outside that job; a job
/// that forbids breaking away gets setup as a normal child instead.
pub fn launch_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        if bloop::update::installer_command(path)
            .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    bloop::update::installer_command(path)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn shortcuts_are_independent_and_keep_old_binding_on_failure() {
        use super::*;
        let mut shortcuts = Shortcuts::default();
        let mut registered = Vec::new();
        let mut change = |add, key| {
            if add {
                registered.push(key);
            } else {
                registered.retain(|k| *k != key);
            }
            Ok(())
        };
        shortcuts.apply(0, "Ctrl+Alt+B", &mut change).unwrap();
        shortcuts.apply(1, "Ctrl+Alt+C", &mut change).unwrap();
        let [Some(toggle), Some(ask)] = shortcuts.0 else {
            panic!("both registered")
        };
        assert!(matches!(shortcuts.event(toggle.id()), Some(Event::Toggle)));
        assert!(matches!(shortcuts.event(ask.id()), Some(Event::Ask)));
        assert!(shortcuts.apply(1, "Ctrl+Alt+B", &mut change).is_err());
        assert!(shortcuts.apply(0, "Ctrl+Alt+C", &mut change).is_err());
        assert!(shortcuts.apply(1, "not a shortcut", &mut change).is_err());
        assert!(
            shortcuts
                .apply(1, "Ctrl+Alt+D", |_, _| Err("occupied".into()))
                .is_err()
        );
        assert_eq!(shortcuts.0, [Some(toggle), Some(ask)]);
        shortcuts.apply(1, "", &mut change).unwrap();
        assert!(shortcuts.event(ask.id()).is_none());
        assert!(matches!(shortcuts.event(toggle.id()), Some(Event::Toggle)));
        shortcuts.apply(1, "", &mut change).unwrap();
        shortcuts.apply(0, "Ctrl+Alt+B", &mut change).unwrap();
        assert_eq!(registered, [toggle]);
    }

    #[test]
    fn placement_uses_physical_origin_and_target_scale() {
        use super::*;
        let area = [-2560, -120, 0, 1320];
        for (scale, width, height, inset) in [
            (1.0, 600, 400, 24),
            (1.5, 900, 600, 36),
            (2.0, 1200, 800, 48),
        ] {
            for (position, x) in [
                (NotchPosition::Centre, -2560 + (2560 - width) / 2),
                (NotchPosition::Left, -2560 + inset),
                (NotchPosition::Right, -inset - width),
            ] {
                assert_eq!(
                    physical_rect(area, scale, Size::new(600.0, 400.0), position),
                    PhysicalRect {
                        x,
                        y: -120,
                        width,
                        height
                    }
                );
            }
        }
        assert_eq!(
            physical_rect(
                [1920, 40, 4480, 1440],
                1.25,
                Size::new(601.0, 401.0),
                NotchPosition::Right
            ),
            PhysicalRect {
                x: 3699,
                y: 40,
                width: 751,
                height: 501
            }
        );
    }
    #[test]
    fn fullscreen_query_does_not_panic() {
        let _ = super::fullscreen_active();
    }

    #[test]
    fn system_reads_do_not_panic() {
        let _ = super::reduce_motion_system();
        let _ = super::light_theme_system();
    }

    #[cfg(windows)]
    #[test]
    fn second_instance_is_refused() {
        let name = r"Local\bloop-test-instance";
        let event = r"Local\bloop-test-open";
        let first = super::instance_named(name, event);
        assert!(first.is_some());
        assert!(super::instance_named(name, event).is_none());
        drop(first);
        assert!(super::instance_named(name, event).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn run_value_round_trip() {
        let key = r"Software\bloop-test\Run";
        super::set_run_value(key, true).unwrap();
        let value = super::run_value(key).expect("value written");
        assert!(value.starts_with('"') && value.ends_with(" --background"));
        let exe = std::env::current_exe().unwrap();
        assert!(value.contains(exe.file_name().unwrap().to_str().unwrap()));
        super::set_run_value(key, false).unwrap();
        assert!(super::run_value(key).is_none());
        super::set_run_value(key, false).unwrap();
        super::delete_tree(r"Software\bloop-test");
    }

    #[cfg(windows)]
    #[test]
    fn migration_rewrites_old_value_and_never_creates_one() {
        use windows::{
            Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW},
            core::PCWSTR,
        };
        let key = r"Software\bloop-test-migrate\Run";
        super::migrate_run_value(key).unwrap();
        assert!(super::run_value(key).is_none());
        let exe = std::env::current_exe().unwrap();
        let old = super::wide(&format!("\"{}\"", exe.display()));
        let (k, n) = (super::wide(key), super::wide(super::RUN_VALUE));
        // SAFETY: strings are NUL-terminated; data pointer and byte length cover the same Vec.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(k.as_ptr()),
                PCWSTR(n.as_ptr()),
                REG_SZ.0,
                Some(old.as_ptr().cast()),
                (old.len() * 2) as u32,
            )
        }
        .ok()
        .unwrap();
        super::migrate_run_value(key).unwrap();
        let value = super::run_value(key).unwrap();
        assert!(value.ends_with(" --background"));
        super::delete_tree(r"Software\bloop-test-migrate");
    }

    #[test]
    fn needs_migration_only_for_a_stale_existing_value() {
        let exe = std::path::Path::new(r"C:\b.exe");
        let current = r#""C:\b.exe" --background"#;
        assert!(!super::needs_migration(None, exe));
        assert!(!super::needs_migration(Some(current), exe));
        assert!(super::needs_migration(Some(r#""C:\b.exe""#), exe));
        assert!(super::needs_migration(Some(r#""c:\B.exe""#), exe));
    }

    #[test]
    fn migration_leaves_another_copys_autostart_alone() {
        // A dev build must not take over the installed copy's sign-in entry, in either form.
        let dev = std::path::Path::new(r"C:\src\bloop\target\debug\bloop.exe");
        let installed = r#""C:\Users\u\AppData\Local\Programs\bloop\bloop.exe""#;
        assert!(!super::needs_migration(Some(installed), dev));
        let installed = r#""C:\Users\u\AppData\Local\Programs\bloop\bloop.exe" --background"#;
        assert!(!super::needs_migration(Some(installed), dev));
    }

    #[test]
    fn run_command_has_background_flag() {
        let cmd = super::run_command(std::path::Path::new(r"C:\Apps\bloop\bloop.exe"));
        assert_eq!(cmd, r#""C:\Apps\bloop\bloop.exe" --background"#);
    }
}

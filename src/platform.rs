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
    #[cfg(not(windows))]
    eprintln!("bloop could not start: {message}");
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Controller),
    Toggle,
    Settings,
    Quit,
    Warning(String),
    ShortcutApplied,
}

#[derive(Debug, Clone)]
pub struct Controller {
    #[cfg(windows)]
    sender: std::sync::mpsc::Sender<Command>,
    #[cfg(windows)]
    thread_id: u32,
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
}

impl Controller {
    pub fn shortcut(&self, value: String) {
        #[cfg(windows)]
        self.send(Command::Shortcut(value));
        #[cfg(not(windows))]
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
    #[cfg(windows)]
    {
        value
            .parse::<global_hotkey::hotkey::HotKey>()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        Ok(())
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
        #[cfg(not(windows))]
        {
            let _ = output
                .send(Event::Warning(
                    "Linux tray and global shortcut are not implemented yet.".into(),
                ))
                .await;
            std::future::pending::<()>().await;
        }
    })
}

#[cfg(windows)]
const WAKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 42;

#[cfg(windows)]
fn run(
    sender: tokio::sync::mpsc::UnboundedSender<Event>,
    commands: std::sync::mpsc::Sender<Command>,
    inbox: std::sync::mpsc::Receiver<Command>,
    state: std::sync::Arc<ThreadState>,
) -> Result<(), String> {
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
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
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = hotkey_sender.send(Event::Toggle);
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
    let mut current: Option<HotKey> = None;
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
                match command {
                    Command::Shortcut(value) => {
                        let result =
                            value
                                .parse::<HotKey>()
                                .map_err(|e| e.to_string())
                                .and_then(|hotkey| {
                                    if current == Some(hotkey) {
                                        return Ok(());
                                    }
                                    manager.register(hotkey).map_err(|e| e.to_string())?;
                                    if let Some(old) = current {
                                        let _ = manager.unregister(old);
                                    }
                                    current = Some(hotkey);
                                    Ok(())
                                });
                        let _ = sender.send(match result {
                            Ok(()) => Event::ShortcutApplied,
                            Err(error) => {
                                Event::Warning(format!("Global shortcut unavailable: {error}"))
                            }
                        });
                    }
                }
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
    #[cfg(not(windows))]
    {
        let _ = (window, outline);
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
    #[cfg(not(windows))]
    {
        false
    }
}

/// Primary monitor work area; physical coordinates converted to logical units.
pub fn primary_position(size: Size) -> Option<Point> {
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
        Some(Point::new(
            (area.left as f32 + (area.right - area.left) as f32 / 2.0) / scale - size.width / 2.0,
            // Flush with the top edge so it reads as a notch.
            area.top as f32 / scale,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = size;
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fullscreen_query_does_not_panic() {
        let _ = super::fullscreen_active();
    }
}

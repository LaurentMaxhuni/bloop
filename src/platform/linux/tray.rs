//! The tray icon over D-Bus (StatusNotifierItem): KDE, most wlroots bars, Xfce, Cinnamon, and
//! GNOME with the AppIndicator extension.

use crate::platform::Event;
use ksni::TrayMethods as _;
use tokio::sync::mpsc::UnboundedSender;

pub const NO_TRAY: &str = "No tray found. On GNOME, enable the AppIndicator extension.";

struct BloopTray {
    sender: UnboundedSender<Event>,
    warned: std::sync::atomic::AtomicBool,
}

impl BloopTray {
    fn warn_once(&self) {
        if !self.warned.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let _ = self.sender.send(Event::Note(NO_TRAY.into()));
        }
    }
}

fn item(label: &str, event: fn() -> Event) -> ksni::MenuItem<BloopTray> {
    ksni::menu::StandardItem {
        label: label.into(),
        activate: Box::new(move |tray: &mut BloopTray| {
            let _ = tray.sender.send(event());
        }),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for BloopTray {
    fn id(&self) -> String {
        "bloop".into()
    }

    fn title(&self) -> String {
        "bloop".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let Some((rgba, width, height)) = super::icon::rgba() else {
            return Vec::new();
        };
        // StatusNotifierItem wants ARGB32 in network byte order.
        let data = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| [a, r, g, b])
            .collect();
        vec![ksni::Icon {
            width: *width as i32,
            height: *height as i32,
            data,
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "bloop · your local AI notch".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(Event::Toggle);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            item("Show / collapse bloop", || Event::Toggle),
            item("Settings", || Event::Settings),
            item("Quit bloop", || Event::Quit),
        ]
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // Keep the service: the tray host may come back (e.g. the panel restarts).
        self.warn_once();
        true
    }
}

/// Runs the tray until bloop exits; warns once if no tray host exists.
pub async fn run(sender: UnboundedSender<Event>) {
    let tray = BloopTray {
        sender: sender.clone(),
        warned: false.into(),
    };
    match tray.spawn().await {
        Ok(handle) => {
            // Keep the handle (and so the tray) alive for the rest of the session.
            std::future::pending::<()>().await;
            drop(handle);
        }
        Err(error) => {
            tracing::info!("tray unavailable: {error}");
            let _ = sender.send(Event::Note(NO_TRAY.into()));
        }
    }
}

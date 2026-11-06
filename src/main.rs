#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod mascot;
mod platform;
#[allow(dead_code)] // wired into the UI in the next task
mod presence;
mod style;
mod ui;

use iced::{Color, Point, Size, window};

fn main() {
    if let Err(error) = run() {
        platform::show_fatal(&error.to_string());
        std::process::exit(1);
    }
}

fn run() -> iced::Result {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bloop=info".into()),
        )
        .try_init();
    let mut settings = window::Settings {
        size: ui::COMPACT,
        position: window::Position::SpecificWith(|size: Size, monitor: Size| {
            platform::primary_position(size)
                .unwrap_or(Point::new((monitor.width - size.width) / 2.0, 0.0))
        }),
        decorations: false,
        transparent: true,
        resizable: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..Default::default()
    };
    #[cfg(windows)]
    {
        settings.platform_specific.skip_taskbar = true;
    }
    iced::application(ui::Island::new, ui::Island::update, ui::Island::view)
        .title("bloop")
        .window(settings)
        .executor::<Executor>()
        .default_font(style::REGULAR)
        .theme(ui::Island::theme)
        .style(|app, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: app.tokens().foreground,
        })
        .subscription(ui::Island::subscription)
        .run()
}

/// Two workers cover one stream and UI tasks without scaling threads to CPU count.
struct Executor(tokio::runtime::Runtime);

impl iced::Executor for Executor {
    fn new() -> Result<Self, std::io::Error> {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .map(Self)
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        std::mem::drop(self.0.spawn(future));
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }
}

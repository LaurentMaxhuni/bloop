#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod icons;
mod mascot;
mod platform;
mod presence;
mod style;
mod ui;

use iced::Color;

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bloop=info".into()),
        )
        .try_init();
    // A second copy (e.g. installer launch while autostart runs) exits quietly.
    let launch = bloop::launch::launch_kind(std::env::args());
    let Some(_instance) = platform::single_instance() else {
        if bloop::launch::signals_running_copy(launch) {
            platform::signal_running_copy();
        }
        return;
    };
    platform::migrate_autostart();
    if let Err(error) = run(launch) {
        platform::show_fatal(&error.to_string());
        std::process::exit(1);
    }
}

fn run(launch: bloop::launch::Launch) -> iced::Result {
    iced::daemon(
        move || ui::Island::new(launch),
        ui::Island::update,
        ui::Island::view,
    )
    .title(ui::Island::title)
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

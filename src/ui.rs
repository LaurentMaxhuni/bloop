use crate::{
    mascot::{self, Mascot, Mood, Notch},
    platform,
    presence::{Event, Presence, Shape, State},
    style::{self, MEDIUM, Tokens, Variant},
};
use bloop::{
    config::{Config, Provider},
    model::{self, MAX_PROMPT, MAX_TURNS, Turn},
    provider::{self, StreamEvent},
    storage,
};
use iced::{
    Alignment, Element, Length, Padding, Size, Subscription, Task, Theme,
    futures::SinkExt,
    widget::{
        self, Space, button, canvas, column, container, pick_list, row, scrollable, stack, text,
        text_input,
    },
    window,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub const SLIVER: Size = Size::new(120.0, 5.0);
pub const COMPACT: Size = Size::new(300.0, 40.0);
const EXPANDED: Size = Size::new(540.0, 580.0);
/// Width of the concave curves where the notch meets the top of the screen.
const SHOULDER: f32 = 10.0;
/// Collapsed and untouched for this long, the mascot dozes off.
const NAP_AFTER: Duration = Duration::from_secs(45);

#[derive(Debug, Clone)]
pub enum Message {
    Window(Option<window::Id>),
    Loaded(Result<(Config, Vec<Turn>), String>),
    Saved(Result<(), String>),
    Platform(platform::Event),
    /// Shortcut-like toggle: opens from anywhere, closes when open.
    Toggle,
    /// Click on the compact pill.
    OpenClick,
    /// Cursor entered (true) or left (false) the window.
    Cursor(bool),
    /// Window gained (true) or lost (false) focus.
    Focus(bool),
    /// A presence deadline (hover/leave/peek) may have passed.
    Deadline(Instant),
    FullscreenPoll,
    PeekTest,
    Frame(Instant),
    /// Mascot animation frame while it is moving.
    Tick(Instant),
    /// Low-frequency wake-up at rest so the mascot can blink.
    Blink(Instant),
    Input(String),
    Send,
    Suggest(&'static str),
    Chunk(u64, StreamEvent),
    Finished(u64, Result<(), String>),
    Cancel,
    Settings,
    Provider(Provider),
    Endpoint(String),
    Model(String),
    Shortcut(String),
    SaveSettings,
    Theme,
    NewChat,
    ConfirmClear,
    KeepChat,
    Quit,
    QuitWithoutSaving,
    Escape,
}

struct Animation {
    start: Instant,
    from: Size,
    to: Size,
}

pub struct Island {
    config: Config,
    draft_config: Config,
    turns: Vec<Turn>,
    input: String,
    status: String,
    desktop_warning: String,
    storage_warning: String,
    loading: bool,
    presence: Presence,
    settings: bool,
    confirming_clear: bool,
    size: Size,
    animation: Option<Animation>,
    window: Option<window::Id>,
    controller: Option<platform::Controller>,
    request_id: u64,
    request: Option<iced::task::Handle>,
    db_path: Option<PathBuf>,
    storage_disabled: bool,
    saving: bool,
    save_again: bool,
    quitting: bool,
    born: Instant,
    now: Instant,
    last_activity: Instant,
    blink_until: Option<Instant>,
    /// A short-lived mood (mood, started, ends) triggered by an event.
    reaction: Option<(Mood, Instant, Instant)>,
}

impl Island {
    pub fn new() -> (Self, Task<Message>) {
        let path = storage::data_path();
        let load_path = path.clone();
        let state = Self {
            config: Config::default(),
            draft_config: Config::default(),
            turns: Vec::new(),
            input: String::new(),
            status: String::new(),
            desktop_warning: String::new(),
            storage_warning: String::new(),
            loading: true,
            presence: Presence::new(),
            settings: false,
            confirming_clear: false,
            size: SLIVER,
            animation: None,
            window: None,
            controller: None,
            request_id: 0,
            request: None,
            db_path: path.ok(),
            storage_disabled: false,
            saving: false,
            save_again: false,
            quitting: false,
            born: Instant::now(),
            now: Instant::now(),
            last_activity: Instant::now(),
            blink_until: None,
            reaction: None,
        };
        (
            state,
            Task::batch([
                window::oldest().map(Message::Window),
                Task::perform(
                    async move {
                        let path = load_path?;
                        tokio::task::spawn_blocking(move || storage::load(&path))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::Loaded,
                ),
            ]),
        )
    }

    pub fn light(&self) -> bool {
        self.config.light
    }
    pub fn theme(&self) -> Theme {
        let light = self.light();
        let t = self.tokens();
        Theme::custom(
            if light { "bloop light" } else { "bloop dark" },
            iced::theme::Palette {
                background: t.background,
                text: t.foreground,
                primary: t.primary,
                danger: t.destructive,
                ..if light {
                    iced::theme::Palette::LIGHT
                } else {
                    iced::theme::Palette::DARK
                }
            },
        )
    }

    pub fn tokens(&self) -> Tokens {
        Tokens::new(self.light())
    }

    /// What the mascot should be feeling right now.
    fn mood(&self) -> Mood {
        if let Some((mood, _, until)) = self.reaction
            && self.now < until
        {
            return mood;
        }
        if self.request.is_some() {
            let waiting = self.turns.last().is_none_or(|t| t.assistant.is_empty());
            return if waiting {
                Mood::Thinking
            } else {
                Mood::Talking
            };
        }
        if !self.is_open() && self.now.saturating_duration_since(self.last_activity) >= NAP_AFTER {
            return Mood::Sleepy;
        }
        if self.is_open() && !self.settings && !self.input.is_empty() {
            return Mood::Typing;
        }
        Mood::Idle
    }

    fn is_open(&self) -> bool {
        self.presence.state() == State::Open
    }

    fn react(&mut self, mood: Mood, seconds: f32) {
        let now = Instant::now();
        self.now = now;
        self.reaction = Some((mood, now, now + Duration::from_secs_f32(seconds)));
    }

    fn mascot(&self, size: f32) -> Element<'_, Message> {
        let t = self.tokens();
        let mood = self.mood();
        let since = self
            .reaction
            .filter(|(m, ..)| *m == mood)
            .map_or(0.0, |(_, start, _)| {
                self.now.saturating_duration_since(start).as_secs_f32()
            });
        canvas(Mascot {
            mood,
            t: self.now.saturating_duration_since(self.born).as_secs_f32(),
            since,
            blink: self.blink_until.is_some_and(|until| self.now < until),
            body: t.foreground,
            face: t.background,
        })
        .width(size)
        .height(size)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut events = vec![
            Subscription::run(platform::events).map(Message::Platform),
            iced::keyboard::listen().filter_map(|event| {
                if matches!(
                    event,
                    iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    }
                ) {
                    Some(Message::Escape)
                } else {
                    None
                }
            }),
            window::events().filter_map(|(_, event)| {
                if matches!(event, window::Event::CloseRequested) {
                    Some(Message::Quit)
                } else {
                    None
                }
            }),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorEntered) => {
                    Some(Message::Cursor(true))
                }
                iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::Cursor(false)),
                iced::Event::Window(window::Event::Focused) => Some(Message::Focus(true)),
                iced::Event::Window(window::Event::Unfocused) => Some(Message::Focus(false)),
                _ => None,
            }),
        ];
        if self.presence.next_deadline().is_some() {
            events.push(iced::time::every(Duration::from_millis(50)).map(Message::Deadline));
        }
        if !self.is_open() {
            events.push(iced::time::every(Duration::from_secs(2)).map(|_| Message::FullscreenPoll));
        }
        if self.animation.is_some() {
            events.push(iced::time::every(Duration::from_millis(16)).map(Message::Frame));
        }
        let blinking = self.blink_until.is_some_and(|until| self.now < until);
        if self.mood().animated() || blinking {
            events.push(iced::time::every(Duration::from_millis(33)).map(Message::Tick));
        } else {
            // At rest: one cheap wake-up every few seconds for a blink / nap check.
            events.push(iced::time::every(Duration::from_millis(3700)).map(Message::Blink));
        }
        Subscription::batch(events)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.quitting && !matches!(message, Message::Saved(_) | Message::Frame(_)) {
            return Task::none();
        }
        if !matches!(
            message,
            Message::Tick(_)
                | Message::Blink(_)
                | Message::Frame(_)
                | Message::Saved(_)
                | Message::Deadline(_)
                | Message::FullscreenPoll
                | Message::Focus(_)
        ) {
            self.last_activity = Instant::now();
            self.now = self.last_activity;
        }
        match message {
            Message::Window(id) => {
                self.window = id;
                return self.resize();
            }
            Message::Tick(now) => {
                self.now = now;
                if self.reaction.is_some_and(|(_, _, until)| now >= until) {
                    self.reaction = None;
                }
                if self.blink_until.is_some_and(|until| now >= until) {
                    self.blink_until = None;
                }
            }
            Message::Blink(now) => {
                self.now = now;
                if self.mood() != Mood::Sleepy {
                    self.blink_until = Some(now + Duration::from_millis(140));
                }
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((config, turns)) => {
                        self.config = config.clone();
                        self.draft_config = config;
                        self.turns = turns;
                    }
                    Err(error) => {
                        self.storage_disabled = true;
                        self.storage_warning =
                            format!("Storage unavailable; changes will not be saved: {error}");
                    }
                }
                if let Some(controller) = &self.controller {
                    controller.shortcut(self.config.shortcut.clone());
                }
            }
            Message::Saved(result) => {
                self.saving = false;
                let failed = result.is_err();
                if let Err(error) = result {
                    self.storage_warning = format!("Could not save: {error}");
                } else {
                    self.storage_warning.clear();
                }
                if self.save_again {
                    self.save_again = false;
                    return self.persist();
                }
                if self.quitting {
                    if !failed {
                        return iced::exit();
                    }
                    self.quitting = false;
                    if !self.is_open() {
                        return self.present(Event::Shortcut);
                    }
                }
            }
            Message::Platform(event) => match event {
                platform::Event::Ready(controller) => {
                    controller.shortcut(self.config.shortcut.clone());
                    self.controller = Some(controller);
                }
                platform::Event::Toggle => return self.present(Event::Shortcut),
                platform::Event::Settings => return self.update(Message::Settings),
                platform::Event::Quit => return self.update(Message::Quit),
                platform::Event::Warning(error) => self.desktop_warning = error,
                platform::Event::ShortcutApplied => self.desktop_warning.clear(),
            },
            Message::Toggle => return self.present(Event::Shortcut),
            Message::OpenClick => return self.present(Event::Click),
            Message::Cursor(true) => return self.present(Event::CursorEntered),
            Message::Cursor(false) => return self.present(Event::CursorLeft),
            Message::Focus(true) => {}
            Message::Focus(false) => return self.present(Event::FocusLost),
            Message::Deadline(now) => return self.present_at(Event::Tick, now),
            Message::FullscreenPoll => {
                return self.present(Event::Fullscreen(platform::fullscreen_active()));
            }
            Message::PeekTest => {
                // Close the panel first so the peek is visible on its own.
                let close = self.present(Event::Dismiss);
                let peek = self.present(Event::Peek("Hi! This is a peek.".into()));
                return Task::batch([close, peek]);
            }
            Message::Frame(now) => {
                if let Some(animation) = &self.animation {
                    let t = (now.duration_since(animation.start).as_secs_f32() / 0.22).min(1.0);
                    let eased = 1.0 - (1.0 - t).powi(3);
                    self.size = Size::new(
                        animation.from.width + (animation.to.width - animation.from.width) * eased,
                        animation.from.height
                            + (animation.to.height - animation.from.height) * eased,
                    );
                    if t >= 1.0 {
                        self.animation = None;
                    }
                    let resize = self.resize();
                    if self.animation.is_none() && self.is_open() && !self.settings {
                        return Task::batch([resize, widget::operation::focus("prompt")]);
                    }
                    return resize;
                }
            }
            Message::Input(value) => {
                if value.len() <= MAX_PROMPT {
                    self.input = value;
                }
            }
            Message::Suggest(prompt) => {
                self.input = prompt.into();
                return self.update(Message::Send);
            }
            Message::Send => {
                if self.loading
                    || self.quitting
                    || self.request.is_some()
                    || self.input.trim().is_empty()
                {
                    return Task::none();
                }
                if let Err(error) = self.config.validate() {
                    self.status = error;
                    self.react(Mood::Oops, 1.2);
                    return Task::none();
                }
                let prompt = self.input.trim().to_owned();
                let messages = model::context(&self.turns, &prompt);
                self.turns.push(Turn {
                    user: prompt,
                    assistant: String::new(),
                    complete: false,
                });
                if self.turns.len() > MAX_TURNS {
                    self.turns.remove(0);
                }
                self.input.clear();
                self.status.clear();
                self.request_id += 1;
                let id = self.request_id;
                let config = self.config.clone();
                let stream = iced::stream::channel(16, async move |mut output| {
                    let result = provider::chat(&config, &messages, |event| {
                        let mut sender = output.clone();
                        async move { sender.send(Message::Chunk(id, event)).await.is_ok() }
                    })
                    .await
                    .map_err(|e| e.to_string());
                    let _ = output.send(Message::Finished(id, result)).await;
                });
                let (task, handle) = Task::run(stream, |message| message).abortable();
                self.request = Some(handle);
                let busy = self.present(Event::Busy(true));
                return Task::batch([task, self.scroll(), busy]);
            }
            Message::Chunk(id, event) if id == self.request_id && self.request.is_some() => {
                if let StreamEvent::Delta(delta) = event {
                    if let Some(turn) = self.turns.last_mut() {
                        turn.assistant.push_str(&delta);
                    }
                    return self.scroll();
                }
            }
            Message::Finished(id, result) if id == self.request_id && self.request.is_some() => {
                self.request = None;
                let mut presence = self.present(Event::Busy(false));
                match result {
                    Ok(()) => {
                        if let Some(turn) = self.turns.last_mut() {
                            turn.complete = true;
                        }
                        self.react(Mood::Happy, 1.6);
                    }
                    Err(error) => {
                        self.status = error;
                        self.react(Mood::Oops, 2.0);
                        let failed = self.present(Event::Peek("Reply failed".into()));
                        presence = Task::batch([presence, failed]);
                    }
                }
                return Task::batch([self.persist(), self.scroll(), presence]);
            }
            Message::Chunk(..) | Message::Finished(..) => {}
            Message::Cancel => {
                self.cancel();
                self.status = "Stopped. Partial reply kept out of model context.".into();
                self.react(Mood::Oops, 0.8);
                let presence = self.present(Event::Busy(false));
                return Task::batch([self.persist(), presence]);
            }
            Message::Settings => {
                self.settings = !self.settings;
                self.draft_config = self.config.clone();
                if !self.is_open() {
                    return self.present(Event::Shortcut);
                }
            }
            Message::Provider(provider) => {
                self.draft_config.provider = provider;
                self.draft_config.endpoint = match provider {
                    Provider::Ollama => "http://127.0.0.1:11434".into(),
                    Provider::OpenAi => "http://127.0.0.1:8080/v1".into(),
                };
            }
            Message::Endpoint(value) => self.draft_config.endpoint = value,
            Message::Model(value) => self.draft_config.model = value,
            Message::Shortcut(value) => self.draft_config.shortcut = value,
            Message::SaveSettings => {
                if self.loading {
                    return Task::none();
                }
                if let Err(error) = self
                    .draft_config
                    .validate()
                    .and_then(|_| platform::validate_shortcut(&self.draft_config.shortcut))
                {
                    self.status = error;
                    self.react(Mood::Oops, 1.2);
                    return Task::none();
                }
                self.config = self.draft_config.clone();
                self.react(Mood::Happy, 1.2);
                self.settings = false;
                self.status.clear();
                if let Some(controller) = &self.controller {
                    controller.shortcut(self.config.shortcut.clone());
                }
                return self.persist();
            }
            Message::Theme => {
                if self.loading {
                    return Task::none();
                }
                self.config.light = !self.config.light;
                self.draft_config.light = self.config.light;
                self.react(Mood::Happy, 0.9);
                return self.persist();
            }
            Message::NewChat => {
                if self.request.is_none() && !self.loading {
                    self.confirming_clear = true;
                }
            }
            Message::KeepChat => self.confirming_clear = false,
            Message::ConfirmClear => {
                if self.request.is_none() && !self.loading {
                    if !self.confirming_clear {
                        return Task::none();
                    }
                    self.confirming_clear = false;
                    self.turns.clear();
                    self.status.clear();
                    return self.persist();
                }
            }
            Message::Quit => {
                self.cancel();
                self.quitting = true;
                let save = self.persist();
                if !self.saving {
                    return iced::exit();
                }
                return save;
            }
            Message::QuitWithoutSaving => {
                self.cancel();
                return iced::exit();
            }
            Message::Escape => {
                if self.request.is_some() {
                    return self.update(Message::Cancel);
                }
                if self.settings {
                    self.settings = false;
                } else if self.is_open() {
                    return self.present(Event::Dismiss);
                }
            }
        }
        Task::none()
    }

    fn cancel(&mut self) {
        if let Some(handle) = self.request.take() {
            handle.abort();
        }
        self.request_id += 1;
    }

    fn persist(&mut self) -> Task<Message> {
        if self.loading || self.storage_disabled {
            return Task::none();
        }
        if self.saving {
            self.save_again = true;
            return Task::none();
        }
        let Some(path) = self.db_path.clone() else {
            return Task::none();
        };
        self.saving = true;
        let (config, turns) = (self.config.clone(), self.turns.clone());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || storage::save(&path, &config, &turns))
                    .await
                    .map_err(|e| e.to_string())?
            },
            Message::Saved,
        )
    }

    fn present(&mut self, event: Event) -> Task<Message> {
        self.present_at(event, Instant::now())
    }

    /// Feeds `event` to the presence state machine and turns its result into
    /// window work: size animation, hide/show, and focus.
    fn present_at(&mut self, event: Event, now: Instant) -> Task<Message> {
        let (state_before, shape_before) = (self.presence.state(), self.presence.shape());
        self.presence.on(event, now);
        let (state, shape) = (self.presence.state(), self.presence.shape());
        let mut tasks = Vec::new();
        if state == State::Hover && state_before != State::Hover {
            self.react(Mood::Perk, 0.6);
        }
        if shape != shape_before {
            self.animation = Some(Animation {
                start: Instant::now(),
                from: self.size,
                to: size_for(shape),
            });
        }
        if let Some(id) = self.window {
            let hidden = state == State::Hidden;
            if hidden != (state_before == State::Hidden) {
                tasks.push(window::set_mode(
                    id,
                    if hidden {
                        window::Mode::Hidden
                    } else {
                        window::Mode::Windowed
                    },
                ));
            }
            if state == State::Open && state_before != State::Open {
                tasks.push(window::gain_focus(id));
                tasks.push(widget::operation::focus("prompt"));
            }
        }
        Task::batch(tasks)
    }

    fn resize(&self) -> Task<Message> {
        let Some(id) = self.window else {
            return Task::none();
        };
        let position = platform::primary_position(self.size);
        let outline = mascot::notch_outline(self.size, self.shoulder(), self.corner_radius());
        Task::batch([
            window::resize(id, self.size),
            window::run(id, move |window| platform::shape(window, &outline)).discard(),
            position
                .map(|point| window::move_to(id, point))
                .unwrap_or_else(Task::none),
        ])
    }

    fn corner_radius(&self) -> f32 {
        (self.size.height / 2.0).min(26.0)
    }

    /// Shoulders shrink with the notch so the 5 px sliver keeps its silhouette.
    fn shoulder(&self) -> f32 {
        (self.size.height * 0.6).clamp(3.0, SHOULDER)
    }

    fn scroll(&self) -> Task<Message> {
        widget::operation::snap_to("conversation", scrollable::RelativeOffset::END)
    }

    fn button<'a>(&self, label: &'a str, variant: Variant) -> button::Button<'a, Message> {
        button(text(label).size(13).font(MEDIUM))
            .padding([7, 12])
            .style(style::btn(self.tokens(), variant))
    }

    fn input<'a>(
        &self,
        placeholder: &'a str,
        value: &'a str,
        on_input: fn(String) -> Message,
    ) -> text_input::TextInput<'a, Message> {
        text_input(placeholder, value)
            .on_input(on_input)
            .padding([8, 12])
            .size(14)
            .style(style::input(self.tokens()))
    }

    fn status_line(&self) -> &'static str {
        match self.mood() {
            Mood::Thinking => "Thinking…",
            Mood::Talking => "Replying…",
            Mood::Happy => "All done",
            Mood::Oops => "Oops",
            Mood::Sleepy => "Napping",
            Mood::Typing => "Listening…",
            Mood::Idle | Mood::Perk => "Ask anything",
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let full = self.is_open() && self.animation.is_none();
        let status = self.presence.message().unwrap_or(self.status_line());
        let content: Element<'_, Message> = if self.size.height < COMPACT.height - 6.0 {
            // Sliver (or mid-animation): just the silhouette.
            Space::new().into()
        } else if full {
            let header = row![
                self.mascot(30.0),
                column![
                    text("bloop").size(15).font(MEDIUM),
                    text(status).size(12).color(t.muted_foreground),
                ],
                Space::new().width(Length::Fill),
                self.button(if self.light() { "Dark" } else { "Light" }, Variant::Ghost)
                    .on_press(Message::Theme),
                self.button(
                    if self.settings { "Back" } else { "Settings" },
                    Variant::Ghost
                )
                .on_press(Message::Settings),
                self.button("Hide", Variant::Ghost)
                    .on_press(Message::Toggle),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            let separator = container(Space::new())
                .width(Length::Fill)
                .height(1)
                .style(style::separator(t));
            let body = if self.settings {
                self.settings_view()
            } else {
                self.chat_view()
            };
            column![header, separator, body]
                .spacing(14)
                .padding(Padding {
                    top: 10.0,
                    bottom: 18.0,
                    left: SHOULDER + 18.0,
                    right: SHOULDER + 18.0,
                })
                .into()
        } else {
            let header = row![
                self.mascot(26.0),
                text("bloop").size(14).font(MEDIUM),
                Space::new().width(Length::Fill),
                text(status).size(12).color(t.muted_foreground),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            button(header)
                .on_press(Message::OpenClick)
                .style(style::btn(t, Variant::Bare))
                .padding(Padding {
                    top: 5.0,
                    bottom: 9.0,
                    left: SHOULDER + 10.0,
                    right: SHOULDER + 18.0,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let shape = canvas(Notch {
            fill: t.background,
            outline: self.light().then_some(t.border),
            shoulder: self.shoulder(),
            radius: self.corner_radius(),
        })
        .width(Length::Fill)
        .height(Length::Fill);
        stack![shape, content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn chat_view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut history = column![].spacing(16).width(Length::Fill);
        if self.turns.is_empty() {
            let suggestion = |label: &'static str, prompt: &'static str| {
                button(
                    row![
                        text(label).size(14),
                        Space::new().width(Length::Fill),
                        text("→").size(14).color(t.muted_foreground),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .on_press(Message::Suggest(prompt))
                .style(style::btn(t, Variant::Outline))
            };
            history = history
                .push(Space::new().height(24))
                .push(text("How can I help?").size(26).font(MEDIUM))
                .push(
                    text("Ask anything. bloop talks to the model you choose, right here in your notch.")
                        .size(14)
                        .color(t.muted_foreground),
                )
                .push(Space::new().height(6))
                .push(suggestion(
                    "Help me untangle an idea",
                    "Help me untangle an idea. Ask me one useful question to get started.",
                ))
                .push(suggestion(
                    "Make something worth writing",
                    "Help me write something. Ask me what I want to say and who it is for.",
                ));
        }
        let last = self.turns.len().saturating_sub(1);
        for (index, turn) in self.turns.iter().enumerate() {
            let bubble = container(text(&turn.user).size(14))
                .padding([8, 12])
                .max_width(380)
                .style(style::bubble(t));
            let mut label = row![
                text("bloop")
                    .size(12)
                    .font(MEDIUM)
                    .color(t.muted_foreground)
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let streaming = index == last && self.request.is_some();
            if !turn.complete && !streaming {
                label = label.push(
                    container(text("Interrupted").size(11))
                        .padding([1, 8])
                        .style(style::badge(t)),
                );
            }
            history = history
                .push(row![Space::new().width(Length::Fill), bubble])
                .push(
                    column![
                        label,
                        text(if turn.assistant.is_empty() {
                            "…"
                        } else {
                            &turn.assistant
                        })
                        .size(14)
                    ]
                    .spacing(4),
                );
        }
        let mut body = column![
            scrollable(history)
                .id("conversation")
                .height(Length::Fill)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(4)
                        .scroller_width(4)
                        .spacing(6),
                ))
                .style(style::scroll(t))
        ]
        .spacing(10);
        if self.confirming_clear {
            body = body.push(
                container(
                    column![
                        text("Clear this conversation?").size(14).font(MEDIUM),
                        text("Your saved history will be removed from this device.")
                            .size(13)
                            .color(t.muted_foreground),
                        row![
                            Space::new().width(Length::Fill),
                            self.button("Keep chat", Variant::Outline)
                                .on_press(Message::KeepChat),
                            self.button("Clear", Variant::Destructive)
                                .on_press(Message::ConfirmClear),
                        ]
                        .spacing(8),
                    ]
                    .spacing(6),
                )
                .padding(14)
                .width(Length::Fill)
                .style(style::card(t, t.background)),
            );
        }
        for warning in [&self.storage_warning, &self.desktop_warning, &self.status] {
            if !warning.is_empty() {
                body = body.push(
                    container(text(warning).size(13).color(t.destructive))
                        .padding([8, 12])
                        .width(Length::Fill)
                        .style(style::card(t, t.background)),
                );
            }
        }
        let input = text_input("What's on your mind?", &self.input)
            .id("prompt")
            .padding([8, 6])
            .size(14)
            .on_input(Message::Input)
            .on_submit(Message::Send)
            .style(style::bare_input(t));
        let action = if self.request.is_some() {
            self.button("Stop", Variant::Secondary)
                .on_press(Message::Cancel)
        } else {
            self.button("Send", Variant::Primary).on_press_maybe(
                (!self.loading && !self.input.trim().is_empty()).then_some(Message::Send),
            )
        };
        let active = !self.input.is_empty();
        body = body.push(
            container(row![input, action].spacing(8).align_y(Alignment::Center))
                .padding(Padding {
                    top: 5.0,
                    bottom: 5.0,
                    left: 8.0,
                    right: 5.0,
                })
                .style(move |_: &Theme| container::Style {
                    background: Some(t.background.into()),
                    border: iced::Border {
                        color: if active { t.ring } else { t.border },
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }),
        );
        let locality = if bloop::config::validate_endpoint(&self.config.endpoint)
            .is_ok_and(|u| u.scheme() == "http")
        {
            "Local"
        } else {
            "Endpoint"
        };
        body.push(
            row![
                container(text(format!("{locality} · {}", self.config.model)).size(11))
                    .padding([2, 8])
                    .style(style::badge(t)),
                Space::new().width(Length::Fill),
                self.button("Clear chat", Variant::Ghost).on_press_maybe(
                    (self.request.is_none() && !self.turns.is_empty()).then_some(Message::NewChat)
                ),
            ]
            .align_y(Alignment::Center),
        )
        .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        fn field<'a>(
            label: &'a str,
            control: impl Into<Element<'a, Message>>,
        ) -> Element<'a, Message> {
            column![text(label).size(13).font(MEDIUM), control.into()]
                .spacing(6)
                .into()
        }
        let mut form = column![
            column![
                text("Settings").size(20).font(MEDIUM),
                text("Choose the model bloop talks to.")
                    .size(13)
                    .color(t.muted_foreground),
            ]
            .spacing(2),
            field(
                "Provider",
                pick_list(
                    [Provider::Ollama, Provider::OpenAi],
                    Some(self.draft_config.provider),
                    Message::Provider,
                )
                .padding([8, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(style::select(t))
                .menu_style(style::menu(t)),
            ),
            field(
                "Endpoint base URL",
                self.input(
                    "http://127.0.0.1:11434",
                    &self.draft_config.endpoint,
                    Message::Endpoint,
                ),
            ),
            field(
                "Model",
                self.input("qwen3:4b", &self.draft_config.model, Message::Model),
            ),
            field(
                "Global shortcut",
                self.input("Ctrl+Alt+B", &self.draft_config.shortcut, Message::Shortcut),
            ),
            text(
                "Cloud credentials come from the BLOOP_API_KEY environment variable and are never saved. Remote endpoints require HTTPS."
            )
            .size(12)
            .color(t.muted_foreground),
        ]
        .spacing(16);
        if !self.status.is_empty() {
            form = form.push(text(&self.status).size(13).color(t.destructive));
        }
        form = form.push(
            row![
                self.button("Save changes", Variant::Primary)
                    .on_press_maybe((!self.loading).then_some(Message::SaveSettings)),
                Space::new().width(Length::Fill),
                self.button("Quit bloop", Variant::Ghost)
                    .on_press(Message::Quit),
            ]
            .align_y(Alignment::Center),
        );
        #[cfg(debug_assertions)]
        {
            form = form.push(
                self.button("Peek test", Variant::Ghost)
                    .on_press(Message::PeekTest),
            );
        }
        if !self.storage_warning.is_empty() {
            form = form
                .push(text(&self.storage_warning).size(13).color(t.destructive))
                .push(
                    self.button("Quit without saving", Variant::Destructive)
                        .on_press(Message::QuitWithoutSaving),
                );
        }
        if !self.desktop_warning.is_empty() {
            form = form.push(text(&self.desktop_warning).size(13).color(t.destructive));
        }
        scrollable(form)
            .height(Length::Fill)
            .style(style::scroll(t))
            .into()
    }
}

fn size_for(shape: Shape) -> Size {
    match shape {
        Shape::Sliver => SLIVER,
        Shape::Compact => COMPACT,
        Shape::Expanded => EXPANDED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Island {
        let (mut app, _) = Island::new();
        app.loading = false;
        app.storage_disabled = true;
        app
    }

    #[test]
    fn cancellation_rejects_queued_chunks_and_completion() {
        let mut app = ready();
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        let _ = app.update(Message::Chunk(id, StreamEvent::Delta("partial".into())));
        let _ = app.update(Message::Cancel);
        let _ = app.update(Message::Chunk(id, StreamEvent::Delta("stale".into())));
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(app.turns[0].assistant, "partial");
        assert!(!app.turns[0].complete);
        assert!(app.request.is_none());
    }

    #[test]
    fn animation_can_reverse_and_stops_at_rest() {
        let mut app = ready();
        assert_eq!(app.size, SLIVER);
        let _ = app.present(Event::Shortcut);
        let start = app.animation.as_ref().unwrap().start;
        let _ = app.update(Message::Frame(start + Duration::from_millis(90)));
        assert!(app.size.width > SLIVER.width && app.size.width < EXPANDED.width);
        let midpoint = app.size;
        let _ = app.present(Event::Shortcut);
        assert_eq!(app.animation.as_ref().unwrap().from, midpoint);
        let start = app.animation.as_ref().unwrap().start;
        let _ = app.update(Message::Frame(start + Duration::from_secs(1)));
        assert_eq!(app.size, SLIVER);
        assert!(app.animation.is_none());
    }

    #[test]
    fn peek_message_shows_in_compact() {
        let mut app = ready();
        let _ = app.update(Message::PeekTest);
        assert_eq!(app.presence.state(), State::Peek);
        assert!(app.presence.message().is_some());
        assert_eq!(app.animation.as_ref().unwrap().to, COMPACT);
    }

    #[test]
    fn send_marks_presence_busy() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        assert_eq!(app.presence.state(), State::Open);
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let _ = app.update(Message::Focus(false));
        assert_eq!(app.presence.state(), State::Peek);
        let id = app.request_id;
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(app.presence.message(), Some("All done"));
    }

    #[test]
    fn hover_perks_mascot() {
        let mut app = ready();
        let _ = app.update(Message::Cursor(true));
        let _ = app.update(Message::Deadline(
            Instant::now() + Duration::from_millis(200),
        ));
        assert_eq!(app.presence.state(), State::Hover);
        assert_eq!(app.mood(), Mood::Perk);
    }

    #[test]
    fn failed_save_keeps_application_open_on_quit() {
        let mut app = ready();
        app.quitting = true;
        app.saving = true;
        let _ = app.update(Message::Saved(Err("disk full".into())));
        assert!(!app.quitting);
        assert_eq!(app.presence.state(), State::Open);
        assert!(app.storage_warning.contains("disk full"));
    }

    #[test]
    fn mascot_reacts_to_reply_lifecycle() {
        let mut app = ready();
        assert_eq!(app.mood(), Mood::Idle);
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        assert_eq!(app.mood(), Mood::Thinking);
        let _ = app.update(Message::Chunk(id, StreamEvent::Delta("hi".into())));
        assert_eq!(app.mood(), Mood::Talking);
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(app.mood(), Mood::Happy);
        let _ = app.update(Message::Tick(Instant::now() + Duration::from_secs(5)));
        assert_eq!(app.mood(), Mood::Idle);
        let _ = app.update(Message::Tick(Instant::now() + NAP_AFTER * 2));
        assert_eq!(app.mood(), Mood::Sleepy);
        let _ = app.update(Message::Toggle);
        assert_ne!(app.mood(), Mood::Sleepy);
    }

    #[test]
    fn send_is_disabled_until_load_finishes() {
        let (mut app, _) = Island::new();
        app.input = "hello".into();
        let _ = app.update(Message::Send);
        assert!(app.turns.is_empty());
    }

    #[test]
    fn clearing_saved_chat_requires_confirmation() {
        let mut app = ready();
        app.turns.push(Turn {
            user: "keep".into(),
            assistant: "saved".into(),
            complete: true,
        });
        let _ = app.update(Message::ConfirmClear);
        assert_eq!(app.turns.len(), 1);
        let _ = app.update(Message::NewChat);
        assert_eq!(app.turns.len(), 1);
        let _ = app.update(Message::KeepChat);
        assert!(!app.confirming_clear);
        let _ = app.update(Message::NewChat);
        let _ = app.update(Message::ConfirmClear);
        assert!(app.turns.is_empty());
    }
}

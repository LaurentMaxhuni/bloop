mod app_window;
mod chats;
mod dashboard;
mod jobs;
mod onboarding;
mod rich;

pub use chats::ChatAction;
pub use dashboard::{AppPage, ProfileEdit};

use crate::{
    icons::{self, Icon},
    mascot::{self, Mascot, Mood, Notch},
    platform,
    presence::{Event, Presence, Shape, State},
    style::{self, MEDIUM, Tokens, Variant},
};
use bloop::{
    chatgpt::{self, AccountStatus},
    config::{
        Accent, Config, ContextMode, Destination, HOVER_DELAYS_MS, Look, NotchPosition, NotchWidth,
        Provider, ReduceMotion, ThemeChoice,
    },
    diagnose,
    launch::{Launch, Startup, startup},
    model::{self, Chat, MAX_PROMPT, MAX_TURNS, Turn},
    motion::{Feel, SizeSpring},
    onboarding::{self as setup, Choice, Detection, Onboarding},
    provider::{self, ChatGptModel, StreamEvent},
    secrets::{self, KeySource},
    storage,
    update::{self, Release, Version},
};
use iced::{
    Alignment, Element, Length, Padding, Size, Subscription, Task, Theme,
    futures::SinkExt,
    widget::{
        self, Space, button, canvas, column, container, pick_list, row, scrollable, stack, text,
        text_editor, text_input,
    },
    window,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Starting window size; `place_window` resizes it to `canvas_size` once it exists.
pub const SLIVER: Size = Size::new(120.0, 5.0);
/// Width of the concave curves where the notch meets the top of the screen.
const SHOULDER: f32 = 10.0;
/// Time between scheduled update checks.
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// First wait after a scheduled check or download fails; it doubles up to [`UPDATE_EVERY`].
/// Sign-in starts bloop before the network is often up, so the first check can fail.
const UPDATE_RETRY: Duration = Duration::from_secs(5 * 60);
/// Collapsed and untouched for this long, the mascot dozes off.
/// The part of reply `index` on screen: all of it, except while the newest one types out
/// (`reveal` bytes of it).
fn shown(turns: &[Turn], reveal: Option<usize>, index: usize) -> &str {
    let Some(turn) = turns.get(index) else {
        return "";
    };
    match reveal {
        Some(end) if index + 1 == turns.len() => {
            turn.assistant.get(..end).unwrap_or(&turn.assistant)
        }
        _ => &turn.assistant,
    }
}

const NAP_AFTER: Duration = Duration::from_secs(45);

/// The notch window: borderless, transparent, always on top and out of the taskbar.
pub fn notch_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: SLIVER,
        position: window::Position::SpecificWith(|size: Size, monitor: Size| {
            platform::primary_position(size)
                .unwrap_or(iced::Point::new((monitor.width - size.width) / 2.0, 0.0))
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
    settings
}

#[derive(Debug, Clone)]
pub enum Message {
    Window(Option<window::Id>),
    Loaded(Result<storage::Loaded, String>),
    /// A queued storage job finished (see `jobs`).
    JobDone(jobs::Done),
    Platform(platform::Event),
    /// Shortcut-like toggle: opens from anywhere, closes when open.
    Toggle,
    /// Click on the compact pill.
    OpenClick,
    /// Cursor entered (true) or left (false) a window.
    Cursor(window::Id, bool),
    /// A window gained (true) or lost (false) focus.
    Focus(window::Id, bool),
    /// Show the app window, opening it if needed.
    OpenWindow,
    /// The user asked to close a window (title-bar X, Alt+F4).
    CloseRequested(window::Id),
    /// A window finished closing.
    WindowClosed(window::Id),
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
    /// Send the last prompt again, replacing its reply.
    Retry,
    /// Put the last prompt back in the input; sending replaces that turn.
    EditLast,
    CancelEdit,
    /// Copy a reply's code block (or a whole reply) to the clipboard.
    CopyCode(String),
    /// A link in a reply was clicked.
    OpenLink(String),
    Provider(Provider),
    Endpoint(String),
    Model(String),
    Instructions(text_editor::Action),
    ChatGptLoaded(Result<AccountStatus, String>),
    ChatGptSignIn(bool),
    ChatGptCancel,
    ChatGptSignedIn(Result<AccountStatus, String>),
    ChatGptSignOut,
    ChatGptSignedOut(Result<(AccountStatus, bool), String>),
    ChatGptRefreshModels,
    ChatGptModels(Result<Vec<ChatGptModel>, String>),
    Shortcut(String),
    SaveSettings,
    Theme,
    /// Start a fresh chat; the current one stays in the chat list.
    NewChat,
    /// Switch the chat's model profile (a chat with history starts a new chat instead).
    PickProfile(u32),
    /// Attach the clipboard's text to the next prompt, opening the notch.
    AskClipboard,
    /// Clipboard text read for the chat with this id.
    Clipboard(i64, Option<String>),
    /// Tie a key saved by an older bloop to the Model page's endpoint.
    BindLegacyKey,
    RemoveAttachment,
    /// Show the chat list in the app window.
    ShowChats,
    Chat(ChatAction),
    EditProfile(u32),
    NewProfile,
    /// Ask before deleting a saved profile.
    DeleteProfile(u32),
    DeleteProfileConfirm,
    DeleteProfileCancel,
    /// Make a profile the one new chats start on; applies and saves at once.
    SetDefaultProfile(u32),
    ProfileName(String),
    AskShortcut(String),
    /// Check the Model page's provider, endpoint and model without sending a chat.
    TestConnection,
    Tested(u64, diagnose::Report),
    Quit,
    QuitWithoutSaving,
    Escape(window::Id),
    /// Up (true) or Down in the notch: step through this chat's earlier prompts.
    Recall(window::Id, bool),
    /// Ctrl+N in the notch.
    NewChatKey(window::Id),
    OnboardNext,
    OnboardBack,
    OnboardSkip,
    Detect,
    Detected(Vec<Choice>),
    PickChoice(Choice),
    ApiKey(String),
    PasteKey,
    KeyPasted(Provider, String, ProfileEdit, Option<String>),
    RemoveKey,
    SetLight(bool),
    SetAutostart(bool),
    ShowSetup,
    /// The icon font finished loading (true) or failed (false: keep word labels).
    FontLoaded(bool),
    /// Show a dashboard page in the app window.
    ShowPage(AppPage),
    SetTheme(ThemeChoice),
    SetContext(ContextMode),
    SetAccent(Accent),
    SetFeel(Feel),
    SetReduceMotion(ReduceMotion),
    SetNotchWidth(NotchWidth),
    SetNotchPosition(NotchPosition),
    SetMascot(bool),
    /// Dress the mascot (Appearance page); applies and saves at once.
    SetLook(Look),
    /// A random hat, glasses, outfit and colour.
    ShuffleLook,
    /// A click on Bloop: a little giggle.
    PokeMascot,
    SetHoverDelay(u16),
    /// Coarse clock for the update schedule.
    UpdateTick(Instant),
    CheckUpdates {
        manual: bool,
    },
    /// Result of a check; `true` when the user asked for it.
    UpdateChecked(bool, Result<Option<Release>, String>),
    UpdateProgress(Version, u8),
    UpdateReady(Result<(Version, PathBuf), String>),
    InstallUpdate,
    SetAutoUpdate(bool),
}

/// Where self-update stands.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    Downloading(Version, u8),
    Ready(Version, PathBuf),
    Failed(String),
}

pub struct Island {
    config: Config,
    draft_config: Config,
    instructions_editor: text_editor::Content,
    turns: Vec<Turn>,
    /// Parsed assistant replies, one per turn (see `sync_replies`).
    replies: Vec<rich::Reply>,
    /// Bytes of the newest reply shown so far while it types out; `None` shows it all.
    /// Streamed text arrives in bursts, so the view reveals it at a steady pace.
    reveal: Option<usize>,
    /// The reaction Bloop has once the newest reply has finished typing out.
    after_reveal: Option<Mood>,
    /// The code block last copied, so its button reads "Copied".
    copied: Option<String>,
    input: String,
    /// The input holds the last prompt for Edit & resend.
    editing: bool,
    /// The input holds the prompt of this turn, recalled with Up; typing clears it.
    recall: Option<usize>,
    status: String,
    desktop_warning: String,
    storage_warning: String,
    loading: bool,
    presence: Presence,
    /// The dashboard page showing in the app window.
    page: AppPage,
    /// The chat in the notch; its turns are `turns`.
    chat: Chat,
    /// Saved chats, newest first (the current one once it has turns).
    chats: Vec<Chat>,
    /// Clipboard text for the next prompt, and whether it was cut to fit.
    attachment: Option<(String, bool)>,
    chat_query: String,
    /// Search results; `None` lists every chat.
    chat_hits: Option<Vec<(Chat, String)>>,
    search_id: u64,
    renaming: Option<(i64, String)>,
    confirm_delete: Option<i64>,
    /// Storage jobs waiting their turn; `saving` is true while one runs.
    jobs: std::collections::VecDeque<jobs::Job>,
    /// Restart to update once the queue is empty.
    install_after_save: bool,
    /// The chat the Chats page asked to open, until the queue has read it. Anything that
    /// changes the notch's chat meanwhile clears it, so a late read can't replace it.
    opening: Option<i64>,
    /// An older bloop's key, not tied to any server, is still saved.
    legacy_key: bool,
    chats_notice: String,
    /// Which model profile the Model page edits.
    profile_edit: ProfileEdit,
    profile_name: String,
    /// The saved profile waiting for a yes before it's deleted.
    confirm_profile_delete: Option<u32>,
    /// The last Test connection result for the fields as they are; cleared when they change.
    connection: Option<diagnose::Report>,
    /// A Test connection is running; its id, so a stale answer is ignored.
    testing: Option<u64>,
    test_id: u64,
    /// Drawn notch size; the OS window stays at `canvas_size` and never animates.
    spring: SizeSpring,
    /// Previous frame time while the spring moves.
    last_frame: Option<Instant>,
    /// Size and offset the click-through region was last cut for, in whole pixels.
    region: Option<(i32, i32, i32)>,
    /// Frame intervals while animating, written to `BLOOP_FRAME_LOG` when it settles.
    frame_log: Vec<f32>,
    /// Windows' "Animation effects" setting is off.
    system_reduce: bool,
    /// Header buttons show icons once the icon font has loaded.
    icons_ready: bool,
    /// The icon font was requested; Iced can only load fonts once a window exists.
    icon_font_requested: bool,
    /// How many times the window was (re)placed; lets tests see display-change handling.
    placements: u32,
    update_state: UpdateState,
    /// The last manual check found nothing newer.
    up_to_date: bool,
    /// When the next scheduled update check is due.
    next_check: Instant,
    /// Scheduled checks or downloads that failed in a row (sets the retry delay).
    update_failures: u32,
    /// The "ready" peek waits for a streaming reply to finish.
    update_peek_pending: bool,
    /// Starts the downloaded installer; replaced in tests.
    installer_launcher: fn(&std::path::Path) -> Result<(), String>,
    /// The notch window.
    window: Option<window::Id>,
    /// The decorated app window, while it is open.
    app_window: Option<window::Id>,
    /// How this process was started; decides which windows open once the config loads.
    launch: Launch,
    controller: Option<platform::Controller>,
    request_id: u64,
    request: Option<iced::task::Handle>,
    db_path: Option<PathBuf>,
    storage_disabled: bool,
    saving: bool,
    quitting: bool,
    born: Instant,
    now: Instant,
    last_activity: Instant,
    blink_until: Option<Instant>,
    /// A short-lived mood (mood, started, ends) triggered by an event.
    reaction: Option<(Mood, Instant, Instant)>,
    /// First-launch setup in progress (shown in the open panel).
    onboarding: Option<Onboarding>,
    detection: Detection,
    autostart: bool,
    autostart_error: String,
    /// Key typed in Settings or setup, saved on Save / Finish; never stored in the config.
    key_draft: String,
    key_status: KeySource,
    chatgpt: AccountStatus,
    chatgpt_busy: bool,
    chatgpt_cancel: Option<tokio::sync::oneshot::Sender<()>>,
    chatgpt_notice: String,
    chatgpt_models: Vec<ChatGptModel>,
    /// Windows' app theme, used when the theme follows the system.
    system_light: bool,
}

impl Island {
    pub fn new(launch: Launch) -> (Self, Task<Message>) {
        let path = storage::data_path();
        let load_path = path.clone();
        let state = Self {
            config: Config::default(),
            draft_config: Config::default(),
            instructions_editor: text_editor::Content::new(),
            turns: Vec::new(),
            replies: Vec::new(),
            reveal: None,
            after_reveal: None,
            copied: None,
            input: String::new(),
            editing: false,
            recall: None,
            status: String::new(),
            desktop_warning: String::new(),
            storage_warning: String::new(),
            loading: true,
            presence: Presence::new(),
            page: AppPage::Home,
            chat: Chat {
                id: model::new_chat_id(0),
                ..Chat::default()
            },
            chats: Vec::new(),
            attachment: None,
            chat_query: String::new(),
            chat_hits: None,
            search_id: 0,
            renaming: None,
            confirm_delete: None,
            jobs: std::collections::VecDeque::new(),
            install_after_save: false,
            opening: None,
            legacy_key: false,
            chats_notice: String::new(),
            profile_edit: ProfileEdit::Default,
            profile_name: String::new(),
            confirm_profile_delete: None,
            connection: None,
            testing: None,
            test_id: 0,
            spring: SizeSpring::new(SLIVER.width, SLIVER.height),
            last_frame: None,
            region: None,
            frame_log: Vec::new(),
            system_reduce: platform::reduce_motion_system(),
            icons_ready: false,
            icon_font_requested: false,
            placements: 0,
            update_state: UpdateState::Idle,
            up_to_date: false,
            next_check: Instant::now() + Duration::from_secs(30),
            update_failures: 0,
            update_peek_pending: false,
            installer_launcher: platform::launch_installer,
            window: None,
            app_window: None,
            launch,
            controller: None,
            request_id: 0,
            request: None,
            db_path: path.ok(),
            storage_disabled: false,
            saving: false,
            quitting: false,
            born: Instant::now(),
            now: Instant::now(),
            last_activity: Instant::now(),
            blink_until: None,
            reaction: None,
            onboarding: None,
            detection: Detection::Checking,
            autostart: false,
            autostart_error: String::new(),
            key_draft: String::new(),
            key_status: KeySource::None,
            chatgpt: AccountStatus::default(),
            chatgpt_busy: true,
            chatgpt_cancel: None,
            chatgpt_notice: String::new(),
            chatgpt_models: Vec::new(),
            system_light: platform::light_theme_system(),
        };
        (
            state,
            Task::batch([
                Task::perform(
                    async move {
                        let path = load_path?;
                        tokio::task::spawn_blocking(move || storage::load(&path))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::Loaded,
                ),
                Task::perform(chatgpt::status(), Message::ChatGptLoaded),
            ]),
        )
    }

    pub fn light(&self) -> bool {
        match self.config.theme {
            ThemeChoice::Light => true,
            ThemeChoice::Dark => false,
            ThemeChoice::System => self.system_light,
        }
    }
    pub fn title(&self, _id: window::Id) -> String {
        "bloop".into()
    }

    /// Both windows share one theme.
    pub fn theme(&self, _id: window::Id) -> Theme {
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
        Tokens::with_accent(self.light(), self.config.accent)
    }

    /// What the mascot should be feeling right now.
    fn mood(&self) -> Mood {
        if let Some((mood, _, until)) = self.reaction
            && self.now < until
        {
            return mood;
        }
        if self.request.is_some() || self.reveal.is_some() {
            return if self.shown_last().is_empty() {
                Mood::Thinking
            } else {
                Mood::Talking
            };
        }
        if !self.is_open() && self.now.saturating_duration_since(self.last_activity) >= NAP_AFTER {
            return Mood::Sleepy;
        }
        if self.is_open() && !self.input.is_empty() {
            return Mood::Typing;
        }
        Mood::Idle
    }

    /// Up fills an empty prompt with this chat's last prompt, then earlier ones; Down walks
    /// back and finally clears it. Only while nothing else uses the prompt. True if it changed.
    fn recall(&mut self, up: bool) -> bool {
        let idle =
            self.is_open() && self.request.is_none() && !self.editing && self.attachment.is_none();
        let current = self.recall.filter(|&i| i < self.turns.len());
        if !idle || (current.is_none() && (!up || !self.input.is_empty())) {
            return false;
        }
        let next = match (current, up) {
            (None, _) => self.turns.len().checked_sub(1),
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) => Some(i + 1).filter(|&n| n < self.turns.len()),
        };
        match next {
            Some(i) => self.input = self.turns[i].user.clone(),
            None => self.input.clear(),
        }
        self.recall = next;
        true
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
        if !self.config.mascot {
            return Space::new().width(0).into();
        }
        let mood = self.mood();
        let since = self.reaction_age(mood);
        let blink = self.blink_until.is_some_and(|until| self.now < until);
        self.draw_mascot(mood, since, blink, size)
    }

    /// Seconds since Bloop started reacting with `mood`, or 0 when it isn't.
    fn reaction_age(&self, mood: Mood) -> f32 {
        self.reaction
            .filter(|(m, ..)| *m == mood)
            .map_or(0.0, |(_, start, _)| {
                self.now.saturating_duration_since(start).as_secs_f32()
            })
    }

    fn draw_mascot(&self, mood: Mood, since: f32, blink: bool, size: f32) -> Element<'_, Message> {
        let t = self.tokens();
        canvas(Mascot {
            mood,
            t: self.now.saturating_duration_since(self.born).as_secs_f32(),
            since,
            blink,
            body: t.foreground,
            face: t.background,
            blush: self.config.accent.rgb().map(|_| t.primary),
            look: self.config.look,
        })
        .width(size)
        .height(size)
        .into()
    }

    /// How the little Bloop beside a reply looks: thinking until text appears, talking
    /// while it types out, then reacting (happy, laughing or mad). It keeps that face
    /// until the chat goes quiet; older replies have dozed off.
    fn reply_mood(&self, index: usize) -> (Mood, f32) {
        // Far past any one-shot motion, so a settled face stands still.
        const SETTLED: f32 = 60.0;
        let Some(turn) = self.turns.get(index) else {
            return (Mood::Sleepy, SETTLED);
        };
        if index + 1 != self.turns.len() {
            return (Mood::Sleepy, SETTLED);
        }
        if self.request.is_some() || self.reveal.is_some() {
            let mood = if self.shown(index).is_empty() {
                Mood::Thinking
            } else {
                Mood::Talking
            };
            return (mood, 0.0);
        }
        if let Some((mood @ (Mood::Happy | Mood::Laughing | Mood::Mad), _, until)) = self.reaction
            && self.now < until
        {
            return (mood, self.reaction_age(mood));
        }
        if self.now.saturating_duration_since(self.last_activity) >= NAP_AFTER {
            return (Mood::Sleepy, SETTLED);
        }
        let mood = Mood::for_reply(&turn.user, &turn.assistant, turn.complete);
        (mood, SETTLED)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut events = vec![
            Subscription::run(platform::events).map(Message::Platform),
            Subscription::run(platform::open_requests).map(|()| Message::OpenWindow),
            window::close_requests().map(Message::CloseRequested),
            window::close_events().map(Message::WindowClosed),
            iced::event::listen_with(|event, status, id| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) if status == iced::event::Status::Ignored => Some(Message::Escape(id)),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key:
                        iced::keyboard::Key::Named(
                            key @ (iced::keyboard::key::Named::ArrowUp
                            | iced::keyboard::key::Named::ArrowDown),
                        ),
                    modifiers,
                    ..
                }) if status == iced::event::Status::Ignored && modifiers.is_empty() => Some(
                    Message::Recall(id, key == iced::keyboard::key::Named::ArrowUp),
                ),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Character(c),
                    modifiers,
                    ..
                }) if c.as_str().eq_ignore_ascii_case("n")
                    && modifiers == iced::keyboard::Modifiers::COMMAND =>
                {
                    Some(Message::NewChatKey(id))
                }
                iced::Event::Mouse(iced::mouse::Event::CursorEntered) => {
                    Some(Message::Cursor(id, true))
                }
                iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    Some(Message::Cursor(id, false))
                }
                iced::Event::Window(window::Event::Focused) => Some(Message::Focus(id, true)),
                iced::Event::Window(window::Event::Unfocused) => Some(Message::Focus(id, false)),
                _ => None,
            }),
        ];
        if self.presence.next_deadline().is_some() {
            events.push(iced::time::every(Duration::from_millis(50)).map(Message::Deadline));
        }
        if !self.is_open() {
            events.push(iced::time::every(Duration::from_secs(2)).map(|_| Message::FullscreenPoll));
        }
        if self.config.auto_update {
            events.push(iced::time::every(Duration::from_secs(15)).map(Message::UpdateTick));
        }
        if self.animating() {
            // Redraw-synced frames, only while the notch is moving.
            events.push(window::frames().map(Message::Frame));
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
        if self.quitting && !matches!(message, Message::JobDone(_) | Message::Frame(_)) {
            return Task::none();
        }
        if !matches!(
            message,
            Message::Tick(_)
                | Message::Blink(_)
                | Message::Frame(_)
                | Message::JobDone(_)
                | Message::Deadline(_)
                | Message::FullscreenPoll
                | Message::Focus(..)
        ) {
            self.last_activity = Instant::now();
            self.now = self.last_activity;
        }
        match message {
            Message::Window(id) => {
                self.window = id;
                return self.place_window(platform::Target::Current);
            }
            Message::Tick(now) => {
                self.now = now;
                if self.reaction.is_some_and(|(_, _, until)| now >= until) {
                    self.reaction = None;
                }
                if self.blink_until.is_some_and(|until| now >= until) {
                    self.blink_until = None;
                }
                if self.reveal.is_some() {
                    self.advance_reveal();
                    return self.scroll();
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
                    Ok(loaded) => {
                        self.config = loaded.config.clone();
                        self.draft_config = loaded.config;
                        self.instructions_editor =
                            text_editor::Content::with_text(&self.draft_config.instructions);
                        self.chats = loaded.chats;
                        match loaded.current {
                            Some((chat, turns)) => {
                                self.chat = chat;
                                self.turns = turns;
                                self.recall = None;
                            }
                            None => {
                                self.chat.id = model::new_chat_id(0);
                                self.chat.profile = self.config.default_profile();
                            }
                        }
                        self.sync_replies();
                    }
                    Err(error) => {
                        self.storage_disabled = true;
                        self.storage_warning =
                            format!("Storage unavailable; changes will not be saved: {error}");
                    }
                }
                self.apply_shortcuts();
                self.presence
                    .set_hover_delay(Duration::from_millis(self.config.hover_delay_ms.into()));
                let ready = self
                    .updates_dir()
                    .and_then(|dir| update::ready(&dir, &Version::current()));
                if let Some((version, path)) = ready {
                    self.update_state = UpdateState::Ready(version, path);
                    // Downloaded before a restart: say so once.
                    self.update_peek_pending = true;
                }
                let layout = self.relayout(false);
                // First-run setup lives in the app window; the notch appears when it ends.
                let (notch, windows) = match startup(self.launch, self.config.onboarded) {
                    Startup::NotchOnly => (self.open_notch(), Task::none()),
                    Startup::Dashboard => (self.open_notch(), self.open_app_window()),
                    Startup::Onboarding => (Task::none(), self.start_setup()),
                };
                return Task::batch([layout, notch, windows]);
            }
            Message::JobDone(done) => return self.job_done(done),
            Message::Platform(event) => match event {
                platform::Event::Ready(controller) => {
                    self.controller = Some(controller);
                    self.apply_shortcuts();
                }
                platform::Event::Toggle => return self.update(Message::Toggle),
                platform::Event::Ask => return self.update(Message::AskClipboard),
                platform::Event::Settings => return self.update(Message::OpenWindow),
                platform::Event::Quit => return self.update(Message::Quit),
                platform::Event::Warning(error) => self.desktop_warning = error,
                platform::Event::ShortcutApplied => self.desktop_warning.clear(),
            },
            // No notch yet (first-run setup): the shortcut brings the setup window forward.
            Message::Toggle if self.window.is_none() => return self.open_app_window(),
            Message::Toggle => return self.present(Event::Shortcut),
            Message::OpenClick => {
                let ready_peek = self
                    .ready_text()
                    .is_some_and(|text| self.presence.message() == Some(text.as_str()));
                if self.presence.state() == State::Peek && ready_peek {
                    return self.update(Message::InstallUpdate);
                }
                return self.present(Event::Click);
            }
            // Only the notch drives presence; the app window's pointer, focus and keys are its own.
            Message::Cursor(id, _) | Message::Focus(id, _) | Message::Escape(id)
                if self.window != Some(id) => {}
            Message::Cursor(_, true) => return self.present(Event::CursorEntered),
            Message::Cursor(_, false) => return self.present(Event::CursorLeft),
            Message::Focus(_, true) => {}
            Message::Focus(_, false) => return self.present(Event::FocusLost),
            Message::OpenWindow => return self.open_app_window(),
            Message::CloseRequested(id) => {
                if self.app_window == Some(id) {
                    self.app_window = None;
                    let close = window::close(id);
                    if self.onboarding.take().is_some() {
                        // Setup stays pending (`onboarded` is still false); the notch takes over.
                        let dismiss = self.present(Event::Dismiss);
                        return Task::batch([close, dismiss, self.open_notch()]);
                    }
                    return close;
                }
                if self.window == Some(id) {
                    return self.update(Message::Quit);
                }
            }
            Message::WindowClosed(id) => {
                if self.app_window == Some(id) {
                    self.app_window = None;
                }
                if self.window == Some(id) {
                    self.window = None;
                }
            }
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
                if !self.animating() {
                    self.last_frame = None;
                    return Task::none();
                }
                let dt = self.last_frame.map_or(1.0 / 60.0, |last| {
                    now.saturating_duration_since(last).as_secs_f32()
                });
                self.last_frame = Some(now);
                if self.reduce_motion() {
                    self.spring.snap();
                } else {
                    self.spring.step(self.config.motion.stiffness(), dt);
                }
                if std::env::var_os("BLOOP_FRAME_LOG").is_some() {
                    self.frame_log.push(dt);
                }
                let region = self.update_region();
                if self.animating() {
                    return region;
                }
                self.last_frame = None;
                self.write_frame_log();
                if self.is_open() {
                    return Task::batch([region, widget::operation::focus("prompt")]);
                }
                return region;
            }
            Message::Input(value) => {
                if value.len() <= MAX_PROMPT {
                    self.input = value;
                    self.recall = None;
                }
            }
            Message::Recall(id, up) => {
                if self.window == Some(id) && self.recall(up) {
                    return widget::operation::move_cursor_to_end("prompt");
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
                    || (self.input.trim().is_empty() && self.attachment.is_none())
                {
                    return Task::none();
                }
                let prompt = match &self.attachment {
                    Some((text, _)) => model::with_attachment(&self.input, text),
                    None => self.input.trim().to_owned(),
                };
                if prompt.len() > MAX_PROMPT {
                    self.status = "Your question and the clipboard text are too long together (8 KB at most). Shorten the question or remove the clipboard text.".into();
                    return Task::none();
                }
                return self.send_prompt(prompt, self.editing);
            }
            Message::Retry => {
                if self.request.is_some() || self.loading || self.quitting {
                    return Task::none();
                }
                let Some(prompt) = self.turns.last().map(|t| t.user.clone()) else {
                    return Task::none();
                };
                return self.send_prompt(prompt, true);
            }
            Message::EditLast => {
                if self.request.is_some() || self.loading {
                    return Task::none();
                }
                if let Some(prompt) = self.turns.last().map(|t| t.user.clone()) {
                    self.finish_reveal();
                    self.input = prompt;
                    self.editing = true;
                    return widget::operation::focus("prompt");
                }
            }
            Message::CancelEdit => {
                self.editing = false;
                self.input.clear();
            }
            Message::Chunk(id, event) if id == self.request_id && self.request.is_some() => {
                if let StreamEvent::Delta(delta) = event {
                    if let Some(turn) = self.turns.last_mut() {
                        turn.assistant.push_str(&delta);
                    }
                    if self.reveal == Some(0) {
                        // Show the first characters straight away; ticks type out the rest.
                        self.advance_reveal();
                    }
                    self.sync_replies();
                    return self.scroll();
                }
            }
            Message::Finished(id, result) if id == self.request_id && self.request.is_some() => {
                self.request = None;
                let mut presence = self.present(Event::Busy(false));
                match result {
                    Ok(()) => {
                        let mut mood = Mood::Happy;
                        if let Some(turn) = self.turns.last_mut() {
                            turn.complete = true;
                            mood = Mood::for_reply(&turn.user, &turn.assistant, true);
                        }
                        // Bloop reacts once the text has finished typing out.
                        self.after_reveal = Some(mood);
                        self.advance_reveal();
                    }
                    Err(error) => {
                        self.status = error;
                        self.finish_reveal();
                        self.react(Mood::Mad, 2.0);
                        let failed = self.present(Event::Peek("Reply failed".into()));
                        presence = Task::batch([presence, failed]);
                    }
                }
                self.touch_chat();
                let update = self.peek_update_if_pending();
                return Task::batch([self.persist(), self.scroll(), presence, update]);
            }
            Message::Chunk(..) | Message::Finished(..) => {}
            Message::CopyCode(code) => {
                self.copied = Some(code.clone());
                return iced::clipboard::write(code);
            }
            Message::OpenLink(url) => {
                if let Err(error) = platform::open_url(&url) {
                    self.status = error;
                }
            }
            Message::Cancel => {
                self.cancel();
                self.status = "Stopped. Partial reply kept out of model context.".into();
                self.react(Mood::Mad, 1.2);
                let update = self.peek_update_if_pending();
                return Task::batch([self.persist(), update]);
            }
            Message::Provider(provider) => {
                if provider == self.draft_config.provider {
                    return Task::none();
                }
                if provider == Provider::ChatGpt || self.draft_config.provider == Provider::ChatGpt
                {
                    self.draft_config.model = if provider == self.config.provider {
                        self.config.model.clone()
                    } else {
                        String::new()
                    };
                }
                self.draft_config.provider = provider;
                self.key_draft.clear();
                self.draft_config.endpoint = if provider == self.config.provider {
                    self.config.endpoint.clone()
                } else {
                    provider.default_endpoint().into()
                };
                self.forget_connection_test();
                self.refresh_key_status();
            }
            Message::Endpoint(value) => {
                let origin = |endpoint: &str| {
                    bloop::config::validate_endpoint(endpoint)
                        .ok()
                        .map(|url| url.origin().ascii_serialization())
                };
                if origin(&value) != origin(&self.draft_config.endpoint) {
                    // A pasted key belongs to the server it was pasted for.
                    self.key_draft.clear();
                }
                self.draft_config.endpoint = value;
                self.forget_connection_test();
                self.refresh_key_status();
            }
            Message::Model(value) => {
                self.draft_config.model = value;
                self.forget_connection_test();
            }
            Message::ChatGptLoaded(result) => {
                self.chatgpt_busy = false;
                match result {
                    Ok(status) => self.chatgpt = status,
                    Err(error) => self.chatgpt_notice = error,
                }
            }
            Message::ChatGptSignIn(new_account) if self.chatgpt_can_manage() => {
                self.chatgpt_busy = true;
                self.chatgpt_notice = "Finish signing in in your browser.".into();
                let (cancel, receive_cancel) = tokio::sync::oneshot::channel();
                self.chatgpt_cancel = Some(cancel);
                return Task::perform(
                    chatgpt::sign_in(platform::open_url, new_account, receive_cancel),
                    Message::ChatGptSignedIn,
                );
            }
            Message::ChatGptCancel => {
                if let Some(cancel) = self.chatgpt_cancel.take() {
                    let _ = cancel.send(());
                }
            }
            Message::ChatGptSignedIn(result) => {
                self.chatgpt_busy = false;
                self.chatgpt_cancel = None;
                match result {
                    Ok(status) => {
                        self.chatgpt = status;
                        self.chatgpt_models.clear();
                        self.chatgpt_notice.clear();
                        if self.chatgpt.plan_enabled {
                            return self.update(Message::ChatGptRefreshModels);
                        }
                    }
                    Err(error) => self.chatgpt_notice = error,
                }
            }
            Message::ChatGptSignOut if self.chatgpt_can_manage() => {
                self.chatgpt_busy = true;
                self.chatgpt_notice = "Signing out of ChatGPT…".into();
                return Task::perform(chatgpt::sign_out(), Message::ChatGptSignedOut);
            }
            Message::ChatGptSignedOut(result) => {
                self.chatgpt_busy = false;
                match result {
                    Ok((status, revoked)) => {
                        self.chatgpt = status;
                        self.chatgpt_models.clear();
                        self.chatgpt_notice = if revoked {
                            "Signed out of ChatGPT.".into()
                        } else {
                            "Signed out of Bloop. Open Manage usage to disconnect Bloop from ChatGPT.".into()
                        };
                    }
                    Err(error) => self.chatgpt_notice = error,
                }
            }
            Message::ChatGptRefreshModels
                if self.chatgpt_can_manage() && self.chatgpt.plan_enabled =>
            {
                self.chatgpt_busy = true;
                self.chatgpt_notice = "Loading available ChatGPT models…".into();
                return Task::perform(
                    async {
                        provider::chatgpt_models()
                            .await
                            .map_err(|error| error.to_string())
                    },
                    Message::ChatGptModels,
                );
            }
            Message::ChatGptModels(result) => {
                self.chatgpt_busy = false;
                match result {
                    Ok(models) => {
                        self.chatgpt_models = models;
                        self.chatgpt_notice = if self.chatgpt_models.is_empty() {
                            "No models are available for this ChatGPT account.".into()
                        } else {
                            String::new()
                        };
                    }
                    Err(error) => self.chatgpt_notice = error,
                }
            }
            Message::ChatGptSignIn(_) | Message::ChatGptSignOut | Message::ChatGptRefreshModels => {
            }
            Message::Shortcut(value) => self.draft_config.shortcut = value,
            Message::Instructions(action) => {
                let edited = action.is_edit();
                self.instructions_editor.perform(action);
                if edited {
                    self.draft_config.instructions = self.instructions_editor.text();
                }
            }
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
                let config = match self.settings_to_save() {
                    Ok(config) => config,
                    Err(error) => {
                        self.status = error;
                        self.react(Mood::Oops, 1.2);
                        return Task::none();
                    }
                };
                if !self.key_draft.trim().is_empty() {
                    if let Err(error) =
                        secrets::set_api_key(&self.draft_config.endpoint, Some(&self.key_draft))
                    {
                        self.status = error;
                        self.react(Mood::Oops, 1.2);
                        return Task::none();
                    }
                    self.key_draft.clear();
                    self.refresh_key_status();
                }
                self.config = config;
                self.saved_profile();
                self.react(Mood::Happy, 1.2);
                self.status.clear();
                self.apply_shortcuts();
                return self.persist();
            }
            Message::Theme => {
                if self.loading {
                    return Task::none();
                }
                self.config.theme = if self.light() {
                    ThemeChoice::Dark
                } else {
                    ThemeChoice::Light
                };
                self.draft_config.theme = self.config.theme;
                self.react(Mood::Happy, 0.9);
                return self.persist();
            }
            Message::NewChatKey(id) => {
                if self.window == Some(id) && self.is_open() && !self.turns.is_empty() {
                    return self.update(Message::NewChat);
                }
            }
            Message::NewChat => {
                if self.request.is_none() && !self.loading {
                    self.start_chat(self.config.default_profile());
                    return widget::operation::focus("prompt");
                }
            }
            Message::PickProfile(id) => {
                if self.request.is_some() || self.loading || id == self.chat.profile {
                    return Task::none();
                }
                if self.turns.is_empty() {
                    self.chat.profile = id;
                    self.chat.destination.clear();
                    self.status.clear();
                } else {
                    // A chat keeps its model, so its history never moves to another one.
                    self.start_chat(id);
                }
                return widget::operation::focus("prompt");
            }
            Message::AskClipboard => {
                if self.window.is_none() {
                    return Task::none();
                }
                let chat = self.chat.id;
                let read = iced::clipboard::read().map(move |text| Message::Clipboard(chat, text));
                if self.is_open() {
                    return read;
                }
                return Task::batch([self.present(Event::Shortcut), read]);
            }
            // A late read for a chat that's no longer in the notch is dropped.
            Message::Clipboard(chat, _) if chat != self.chat.id => {}
            Message::Clipboard(_, content) => {
                match content.filter(|text| !text.trim().is_empty()) {
                    Some(text) => {
                        self.attachment = Some(model::clip_attachment(&text));
                        self.status.clear();
                    }
                    None => self.status = "The clipboard has no text to attach.".into(),
                }
                return widget::operation::focus("prompt");
            }
            Message::RemoveAttachment => self.attachment = None,
            Message::ShowChats => {
                self.page = AppPage::Chats;
                let window = self.open_app_window();
                self.page = AppPage::Chats;
                return window;
            }
            Message::Chat(action) => return self.chat_action(action),
            Message::EditProfile(id) => self.edit_profile(id),
            Message::NewProfile => self.new_profile(),
            Message::DeleteProfile(id) => self.confirm_profile_delete = Some(id),
            Message::DeleteProfileCancel => self.confirm_profile_delete = None,
            Message::DeleteProfileConfirm => return self.delete_profile(),
            Message::SetDefaultProfile(id) => return self.set_default_profile(id),
            Message::ProfileName(name) => {
                if name.chars().count() <= 40 {
                    self.profile_name = name;
                }
            }
            Message::AskShortcut(value) => self.draft_config.ask_shortcut = value,
            Message::BindLegacyKey => {
                if let Err(error) = secrets::bind_legacy_key(&self.draft_config.endpoint) {
                    self.status = error;
                }
                self.refresh_key_status();
            }
            Message::TestConnection => {
                self.test_id += 1;
                let id = self.test_id;
                self.testing = Some(id);
                self.connection = None;
                let config = self.draft_config.clone();
                return Task::perform(
                    async move { diagnose::test_connection(&config).await },
                    move |report| Message::Tested(id, report),
                );
            }
            Message::Tested(id, report) => {
                if self.testing == Some(id) {
                    self.testing = None;
                    self.connection = Some(report);
                }
            }
            Message::Quit => {
                self.cancel();
                self.quitting = true;
                let save = self.persist();
                if self.idle_storage() {
                    return iced::exit();
                }
                return save;
            }
            Message::QuitWithoutSaving => {
                self.cancel();
                return iced::exit();
            }
            Message::OnboardNext => {
                if let Some(onboarding) = &mut self.onboarding {
                    if onboarding.is_last() {
                        return self.finish_onboarding();
                    }
                    onboarding.next();
                }
            }
            Message::OnboardBack => {
                if let Some(onboarding) = &mut self.onboarding {
                    onboarding.back();
                }
            }
            Message::OnboardSkip => {
                if self.onboarding.is_some() {
                    return self.finish_onboarding();
                }
            }
            Message::Detect => {
                if self.onboarding.is_some() {
                    self.detection = Detection::Checking;
                    if !self.key_draft.trim().is_empty() {
                        // A key pasted for Unsloth is needed before it will list models.
                        let _ = secrets::set_api_key(
                            Provider::Unsloth.default_endpoint(),
                            Some(&self.key_draft),
                        );
                        self.key_draft.clear();
                        self.refresh_key_status();
                    }
                    return Task::perform(setup::detect(), Message::Detected);
                }
            }
            Message::Detected(choices) => {
                if self.onboarding.is_none() {
                    return Task::none();
                }
                if choices.is_empty() {
                    self.detection = Detection::NotFound;
                } else {
                    if self.draft_config.provider != Provider::ChatGpt
                        && let Some(index) = setup::preselect(&choices, &self.draft_config)
                    {
                        let pick = choices[index].clone();
                        self.apply_choice(&pick);
                    }
                    self.detection = Detection::Found(choices);
                }
            }
            Message::PickChoice(choice) => self.apply_choice(&choice),
            Message::ApiKey(value) => self.key_draft = value,
            Message::PasteKey => {
                let provider = self.draft_config.provider;
                let endpoint = self.draft_config.endpoint.clone();
                let profile = self.profile_edit;
                return iced::clipboard::read().map(move |text| {
                    Message::KeyPasted(provider, endpoint.clone(), profile, text)
                });
            }
            Message::KeyPasted(provider, endpoint, profile, value) => {
                // A delayed read must not paste a server's key into another profile.
                if provider == self.draft_config.provider
                    && endpoint == self.draft_config.endpoint
                    && profile == self.profile_edit
                    && matches!(provider, Provider::OpenAi | Provider::Unsloth)
                    && self.key_status != KeySource::Environment
                    && let Some(value) = value
                {
                    self.key_draft = value.trim().to_owned();
                }
            }
            Message::RemoveKey => {
                if let Err(error) = secrets::set_api_key(&self.draft_config.endpoint, None) {
                    self.status = error;
                }
                self.key_draft.clear();
                self.refresh_key_status();
            }
            Message::SetLight(light) => {
                self.config.theme = if light {
                    ThemeChoice::Light
                } else {
                    ThemeChoice::Dark
                };
                self.draft_config.theme = self.config.theme;
            }
            Message::SetAutostart(enable) => match platform::set_autostart(enable) {
                Ok(()) => {
                    self.autostart = enable;
                    self.autostart_error.clear();
                }
                Err(error) => self.autostart_error = error,
            },
            Message::FontLoaded(ok) => self.icons_ready = ok,
            Message::UpdateTick(now) => {
                let idle = matches!(
                    self.update_state,
                    UpdateState::Idle | UpdateState::Failed(_)
                );
                if self.config.auto_update && idle && now >= self.next_check {
                    self.next_check = now + UPDATE_EVERY;
                    return self.update(Message::CheckUpdates { manual: false });
                }
            }
            Message::CheckUpdates { manual } => {
                if matches!(
                    self.update_state,
                    UpdateState::Checking | UpdateState::Downloading(..) | UpdateState::Ready(..)
                ) {
                    return Task::none();
                }
                self.update_state = UpdateState::Checking;
                self.up_to_date = false;
                return Task::perform(
                    async {
                        update::latest(&Version::current())
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::UpdateChecked(manual, result),
                );
            }
            Message::UpdateChecked(manual, result) => match result {
                Ok(Some(release)) => return self.start_download(release),
                Ok(None) => {
                    self.update_failures = 0;
                    self.update_state = UpdateState::Idle;
                    self.up_to_date = manual;
                }
                Err(error) => {
                    // Scheduled checks fail quietly and try again soon (see `UPDATE_RETRY`).
                    self.update_state = if manual {
                        UpdateState::Failed(format!("Couldn't check for updates: {error}"))
                    } else {
                        UpdateState::Idle
                    };
                    self.retry_update_later(Instant::now());
                }
            },
            Message::UpdateProgress(version, percent) => {
                if let UpdateState::Downloading(current, _) = &self.update_state
                    && *current == version
                {
                    self.update_state = UpdateState::Downloading(version, percent);
                }
            }
            Message::UpdateReady(result) => match result {
                Ok((version, path)) => {
                    self.update_failures = 0;
                    self.update_state = UpdateState::Ready(version, path);
                    self.update_peek_pending = true;
                    return self.peek_update_if_pending();
                }
                Err(error) => {
                    self.update_state =
                        UpdateState::Failed(format!("Couldn't download the update: {error}"));
                    self.retry_update_later(Instant::now());
                }
            },
            Message::InstallUpdate => return self.install_update(),
            Message::SetAutoUpdate(on) => {
                if on {
                    self.next_check = Instant::now();
                }
                return self.apply_live(|c| c.auto_update = on);
            }
            Message::ShowPage(page) => self.page = page,
            Message::SetTheme(theme) => return self.apply_live(|c| c.theme = theme),
            Message::SetContext(context) => return self.apply_live(|c| c.context = context),
            Message::SetAccent(accent) => return self.apply_live(|c| c.accent = accent),
            Message::SetFeel(feel) => return self.apply_live(|c| c.motion = feel),
            Message::SetReduceMotion(choice) => {
                return self.apply_live(|c| c.reduce_motion = choice);
            }
            Message::SetMascot(on) => return self.apply_live(|c| c.mascot = on),
            Message::SetLook(look) => {
                self.react(Mood::Happy, 1.6);
                return self.apply_live(move |c| c.look = look);
            }
            Message::ShuffleLook => {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let look = self.config.look.shuffled(seed);
                return self.update(Message::SetLook(look));
            }
            Message::PokeMascot => {
                if self.request.is_none() {
                    self.react(Mood::Laughing, 2.4);
                }
            }
            Message::SetHoverDelay(ms) => {
                if !HOVER_DELAYS_MS.contains(&ms) {
                    return Task::none();
                }
                self.presence
                    .set_hover_delay(Duration::from_millis(ms.into()));
                return self.apply_live(|c| c.hover_delay_ms = ms);
            }
            Message::SetNotchWidth(width) => {
                let save = self.apply_live(|c| c.notch_width = width);
                return Task::batch([save, self.relayout(true)]);
            }
            Message::SetNotchPosition(position) => {
                let save = self.apply_live(|c| c.notch_position = position);
                return Task::batch([save, self.relayout(true)]);
            }
            Message::ShowSetup => {
                self.config.onboarded = false;
                let save = self.persist();
                return Task::batch([save, self.start_setup()]);
            }
            Message::Escape(_) => {
                if self.request.is_some() {
                    return self.update(Message::Cancel);
                }
                if self.editing {
                    return self.update(Message::CancelEdit);
                }
                if self.is_open() {
                    return self.present(Event::Dismiss);
                }
            }
        }
        Task::none()
    }

    /// Shows step one of setup in the app window and starts looking for a model server.
    fn start_setup(&mut self) -> Task<Message> {
        let window = self.open_app_window();
        self.onboarding = Some(Onboarding::new());
        self.draft_config = self.config.clone();
        self.instructions_editor = text_editor::Content::with_text(&self.draft_config.instructions);
        self.autostart = platform::autostart();
        self.autostart_error.clear();
        self.key_draft.clear();
        self.profile_edit = ProfileEdit::Default;
        self.refresh_key_status();
        Task::batch([window, self.update(Message::Detect)])
    }

    /// Ends setup. A valid draft is adopted; an invalid one is dropped so finishing never blocks.
    fn finish_onboarding(&mut self) -> Task<Message> {
        self.onboarding = None;
        if !self.key_draft.trim().is_empty() {
            // Finishing never blocks; a failed save shows in Settings.
            if let Err(error) =
                secrets::set_api_key(&self.draft_config.endpoint, Some(&self.key_draft))
            {
                self.status = error;
            }
            self.key_draft.clear();
        }
        let theme = self.config.theme;
        if self
            .draft_config
            .validate()
            .and_then(|_| platform::validate_shortcut(&self.draft_config.shortcut))
            .is_ok()
        {
            self.config = self.draft_config.clone();
            self.apply_shortcuts();
        }
        self.config.theme = theme;
        self.config.onboarded = true;
        self.draft_config = self.config.clone();
        self.instructions_editor = text_editor::Content::with_text(&self.draft_config.instructions);
        self.status.clear();
        self.page = AppPage::Home;
        self.react(Mood::Happy, 1.2);
        let save = self.persist();
        let notch = self.open_notch();
        let close = self.present(Event::Dismiss);
        let peek = self.present(Event::Peek("I'm up here!".into()));
        Task::batch([save, notch, close, peek])
    }

    /// Where replies come from, shown under the prompt: the profile's name once more than
    /// one exists, Local or Cloud, then the provider and model.
    fn badge_text(&self) -> String {
        match self.config.with_profile(self.chat.profile) {
            Some(config) => self.describe_model(&config, Some(self.chat.profile)),
            None => "Profile removed".into(),
        }
    }

    /// "Local · qwen3:4b", "Cloud · ChatGPT · gpt-5", "Work · Cloud · Codex". `profile`
    /// `None` leaves the name out.
    fn describe_model(&self, config: &Config, profile: Option<u32>) -> String {
        let mut parts = Vec::new();
        if !self.config.profiles.is_empty()
            && let Some(name) = profile.and_then(|id| self.config.profile_name(id))
        {
            parts.push(name.to_owned());
        }
        let destination = config.destination();
        parts.push(destination.label().to_owned());
        match config.provider {
            Provider::ChatGpt if self.chatgpt.plan_enabled => parts.push("ChatGPT plan".into()),
            provider if provider.is_cli() || provider == Provider::ChatGpt => {
                parts.push(provider.to_string());
            }
            _ if destination != Destination::Local => parts.push(destination.describe()),
            _ => {}
        }
        let model = config.model.trim();
        if !model.is_empty() {
            parts.push(model.to_owned());
        }
        parts.join(" · ")
    }

    /// `id` when that profile still exists, else the default.
    fn usable_profile(&self, id: u32) -> u32 {
        if self.config.profile_name(id).is_some() {
            id
        } else {
            self.config.default_profile()
        }
    }

    /// Replaces the notch's chat with a new, empty one on `profile`. The old chat was saved
    /// with its last reply, so nothing is lost.
    fn start_chat(&mut self, profile: u32) {
        let newest = self
            .chats
            .iter()
            .map(|c| c.id)
            .max()
            .unwrap_or(0)
            .max(self.chat.id);
        self.chat = Chat {
            id: model::new_chat_id(newest),
            profile,
            ..Chat::default()
        };
        self.opening = None;
        self.turns.clear();
        self.recall = None;
        self.replies.clear();
        self.finish_reveal();
        self.editing = false;
        self.attachment = None;
        self.input.clear();
        self.status.clear();
    }

    /// Puts the current chat at the top of the chat list once it has turns.
    fn touch_chat(&mut self) {
        self.chat.updated = model::unix_seconds();
        self.chats.retain(|c| c.id != self.chat.id);
        if !self.turns.is_empty() {
            self.chats.insert(0, self.chat.clone());
        }
    }

    /// Registers both global shortcuts with the platform thread.
    fn apply_shortcuts(&self) {
        if let Some(controller) = &self.controller {
            controller.shortcut(self.config.shortcut.clone());
            controller.ask_shortcut(self.config.ask_shortcut.trim().to_owned());
        }
    }

    fn forget_connection_test(&mut self) {
        self.connection = None;
        self.testing = None;
    }

    fn refresh_key_status(&mut self) {
        self.key_status = secrets::key_source(&self.draft_config.endpoint);
        self.legacy_key = secrets::has_legacy_key();
    }

    /// Sends `prompt` as a new turn. With `replace_last`, the last turn is dropped first
    /// (Retry, Edit & resend), so it is neither shown twice nor sent back as context.
    fn send_prompt(&mut self, prompt: String, replace_last: bool) -> Task<Message> {
        self.recall = None;
        let Some(config) = self.config.with_profile(self.chat.profile) else {
            self.status =
                "This chat's model profile was deleted. Start a new chat to keep going.".into();
            return Task::none();
        };
        if let Err(error) = config.validate() {
            self.status = error;
            self.react(Mood::Oops, 1.2);
            return Task::none();
        }
        let destination = config.destination();
        if !self.turns.is_empty() && self.chat.destination == model::UNKNOWN_DESTINATION {
            self.status = "This chat comes from an older bloop, which didn't record where it was sent, so bloop won't send it anywhere. Start a new chat to keep going.".into();
            return Task::none();
        }
        if !self.turns.is_empty()
            && !self.chat.destination.is_empty()
            && self.chat.destination != destination.key()
        {
            let before = Destination::from_key(&self.chat.destination);
            self.status = format!(
                "This chat's history went to {}, and bloop won't send it to {}. Start a new chat to use this model.",
                before.describe(),
                destination.describe()
            );
            self.react(Mood::Oops, 1.2);
            return Task::none();
        }
        if config.provider == Provider::ChatGpt {
            if self.chatgpt_busy {
                self.status = "Finish connecting to ChatGPT before sending a message.".into();
                return Task::none();
            }
            if !self.chatgpt.plan_enabled {
                self.status = "Connect your ChatGPT plan under Settings → Model.".into();
                return Task::none();
            }
        }
        self.finish_reveal();
        if replace_last {
            self.turns.pop();
            self.replies.truncate(self.turns.len());
        }
        self.editing = false;
        self.attachment = None;
        if self.turns.is_empty() {
            self.chat.destination = destination.key();
        }
        if self.chat.title.is_empty() {
            self.chat.title = model::title_for(&prompt);
        }
        let messages = model::context(
            &self.turns,
            &prompt,
            &config.instructions,
            config.context.max_turns(),
        );
        self.turns.push(Turn {
            user: prompt,
            assistant: String::new(),
            complete: false,
        });
        if self.turns.len() > MAX_TURNS {
            self.turns.remove(0);
            if !self.replies.is_empty() {
                self.replies.remove(0);
            }
        }
        self.reveal = (!self.reduce_motion()).then_some(0);
        self.sync_replies();
        self.input.clear();
        self.status.clear();
        self.opening = None;
        self.touch_chat();
        self.request_id += 1;
        let id = self.request_id;
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
        Task::batch([task, self.scroll(), busy])
    }

    fn chatgpt_can_manage(&self) -> bool {
        !self.loading && !self.chatgpt_busy && self.request.is_none()
    }

    fn apply_choice(&mut self, choice: &Choice) {
        if choice.provider != self.draft_config.provider
            || choice.endpoint != self.draft_config.endpoint
        {
            // A pasted key belongs to the server it was pasted for.
            self.key_draft.clear();
            self.forget_connection_test();
        }
        // Re-picking the CLI already in use keeps the model the user typed for it.
        if !(choice.provider.is_cli() && choice.provider == self.draft_config.provider) {
            self.draft_config.model = choice.model.clone();
        }
        self.draft_config.provider = choice.provider;
        self.draft_config.endpoint = choice.endpoint.clone();
        self.refresh_key_status();
    }

    /// API key field for providers that take one, or where the key comes from.
    fn key_field(&self) -> Option<Element<'_, Message>> {
        if !matches!(
            self.draft_config.provider,
            Provider::OpenAi | Provider::Unsloth
        ) {
            return None;
        }
        let t = self.tokens();
        let note = |copy: &'static str| text(copy).size(12).color(t.muted_foreground);
        let control: Element<'_, Message> = match self.key_status {
            KeySource::Environment => note("Using BLOOP_API_KEY from the environment").into(),
            status => {
                let input = self
                    .input(
                        if status == KeySource::Saved {
                            "Paste a new key to replace it"
                        } else {
                            "Paste the key (sk-…)"
                        },
                        &self.key_draft,
                        Message::ApiKey,
                    )
                    .secure(true);
                let mut column = column![
                    row![
                        input,
                        self.small_button(
                            Icon::ClipboardPaste,
                            "Paste key",
                            "Paste key",
                            Message::PasteKey,
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                ]
                .spacing(6);
                if status == KeySource::None && self.legacy_key {
                    column = column.push(
                        row![
                            note("An older bloop saved a key without a server. Use it here?")
                                .width(Length::Fill),
                            self.button("Use it for this server", Variant::Ghost)
                                .on_press(Message::BindLegacyKey),
                        ]
                        .align_y(Alignment::Center),
                    );
                }
                if status == KeySource::Saved {
                    column = column.push(
                        row![
                            note("Saved in Windows Credential Manager"),
                            Space::new().width(Length::Fill),
                            self.icon_action(
                                Icon::Trash,
                                "Remove key",
                                "Remove the saved key",
                                Variant::Ghost,
                                Some(Message::RemoveKey),
                            ),
                        ]
                        .align_y(Alignment::Center),
                    );
                }
                column.into()
            }
        };
        Some(field("API key", control))
    }

    fn cancel(&mut self) {
        if let Some(handle) = self.request.take() {
            handle.abort();
        }
        self.request_id += 1;
        self.finish_reveal();
        // Only state changes matter here; this event never shows, hides or focuses the window.
        let _ = self.present(Event::Cancelled);
    }

    fn persist(&mut self) -> Task<Message> {
        if self.loading || self.storage_disabled {
            return Task::none();
        }
        if self.db_path.is_none() {
            return Task::none();
        }
        let chat = (!self.turns.is_empty()).then(|| (self.chat.clone(), self.turns.clone()));
        self.queue(jobs::Job::Save {
            config: Box::new(self.config.clone()),
            chat,
        })
    }

    fn present(&mut self, event: Event) -> Task<Message> {
        self.present_at(event, Instant::now())
    }

    /// Feeds `event` to the presence state machine and turns its result into
    /// window work: size animation, hide/show, and focus.
    fn present_at(&mut self, event: Event, now: Instant) -> Task<Message> {
        let (state_before, shape_before) = (self.presence.state(), self.presence.shape());
        let target = if event == Event::Shortcut {
            platform::Target::Active
        } else {
            platform::Target::Current
        };
        self.presence.on(event, now);
        let (state, shape) = (self.presence.state(), self.presence.shape());
        let mut tasks = Vec::new();
        if state == State::Hover && state_before != State::Hover {
            self.react(Mood::Perk, 0.6);
        }
        if state == State::Open && state_before != State::Open {
            // "Follow Windows" picks up a theme switch the next time the panel opens.
            self.system_light = platform::light_theme_system();
        }
        if shape != shape_before {
            if shape_before == Shape::Sliver {
                // Coming out of the sliver: pick up resolution, scaling or taskbar changes.
                tasks.push(self.place_window(target));
            }
            self.retarget(shape, now);
            if self.reduce_motion() {
                tasks.push(self.update_region());
            }
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
        let was_away = matches!(state_before, State::Open | State::Hidden);
        if was_away && !matches!(state, State::Open | State::Hidden) {
            // The panel closed or the notch came back: a waiting "update ready" can show now.
            tasks.push(self.peek_update_if_pending());
        }
        Task::batch(tasks)
    }

    /// Drawn notch size right now.
    pub fn size(&self) -> Size {
        let (w, h) = self.spring.size();
        Size::new(w, h)
    }

    /// Notch size for each presence shape, from the notch width setting.
    pub fn size_for(&self, shape: Shape) -> Size {
        let (w, h) = match shape {
            Shape::Sliver => NotchWidth::SLIVER,
            Shape::Compact => self.config.notch_width.compact(),
            Shape::Expanded => self.config.notch_width.panel(),
        };
        Size::new(w, h)
    }

    /// The fixed window: big enough for the open panel.
    pub fn canvas_size(&self) -> Size {
        // Always the widest panel: width changes then never resize the window, so the notch
        // stays inside it while it animates to the new width.
        let (w, h) = NotchWidth::L.panel();
        Size::new(w, h)
    }

    /// Where a rect of `size` sits horizontally in the canvas for the notch position.
    fn x_in_canvas(&self, width: f32) -> f32 {
        let canvas = self.canvas_size().width;
        match self.config.notch_position {
            NotchPosition::Centre => (canvas - width) / 2.0,
            NotchPosition::Left => 0.0,
            NotchPosition::Right => canvas - width,
        }
    }

    /// Where the content is laid out: at the shape's final size, not the animated one, so
    /// text and controls never re-flow while the notch grows. The window region (cut to
    /// the drawn outline) hides whatever is outside the notch.
    pub fn content_rect(&self) -> iced::Rectangle {
        let size = self.size_for(self.presence.shape());
        iced::Rectangle::new(iced::Point::new(self.x_in_canvas(size.width), 0.0), size)
    }

    /// Where the notch is drawn inside the fixed window: top-aligned, placed per position.
    pub fn notch_rect(&self) -> iced::Rectangle {
        let size = self.size();
        iced::Rectangle::new(iced::Point::new(self.x_in_canvas(size.width), 0.0), size)
    }

    pub fn animating(&self) -> bool {
        !self.spring.settled()
    }

    fn reduce_motion(&self) -> bool {
        match self.config.reduce_motion {
            ReduceMotion::System => self.system_reduce,
            ReduceMotion::On => true,
            ReduceMotion::Off => false,
        }
    }

    /// Sizes and places the fixed window, then cuts its click-through region.
    /// Applies an appearance change to the saved and the draft config and saves it now.
    fn updates_dir(&self) -> Option<PathBuf> {
        Some(self.db_path.as_ref()?.parent()?.join("updates"))
    }

    /// What the pill says: a peek's message, else "Update X ready" when nothing more
    /// important (a warning) is pending, else the mascot's status.
    pub fn pill_status(&self) -> String {
        if let Some(message) = self.presence.message() {
            return message.to_owned();
        }
        let quiet = self.status.is_empty()
            && self.storage_warning.is_empty()
            && self.desktop_warning.is_empty();
        match &self.update_state {
            UpdateState::Ready(version, _) if quiet => format!("Update {version} ready"),
            _ => self.status_line().to_owned(),
        }
    }

    fn ready_text(&self) -> Option<String> {
        match &self.update_state {
            UpdateState::Ready(version, _) => {
                Some(format!("Update {version} ready — click to restart"))
            }
            _ => None,
        }
    }

    /// One line for the dashboard's update status.
    pub fn update_status(&self) -> String {
        match &self.update_state {
            UpdateState::Idle if self.up_to_date => "Up to date".into(),
            UpdateState::Idle => String::new(),
            UpdateState::Checking => "Checking…".into(),
            UpdateState::Downloading(version, percent) => {
                format!("Downloading {version} ({percent} %)")
            }
            UpdateState::Ready(version, _) => format!("{version} ready"),
            UpdateState::Failed(error) => error.clone(),
        }
    }

    /// After a failed check or download: the next scheduled check comes 5 min later, then
    /// 10, 20, … up to the usual six hours, instead of a full six hours after a blip.
    fn retry_update_later(&mut self, now: Instant) {
        let factor = 2u32.saturating_pow(self.update_failures.min(16));
        let delay = UPDATE_RETRY.saturating_mul(factor).min(UPDATE_EVERY);
        self.update_failures = self.update_failures.saturating_add(1);
        self.next_check = self.next_check.min(now + delay);
    }

    fn start_download(&mut self, release: Release) -> Task<Message> {
        let Some(dir) = self.updates_dir() else {
            self.update_state =
                UpdateState::Failed("Couldn't download the update: no data folder.".into());
            return Task::none();
        };
        let version = release.version;
        self.update_state = UpdateState::Downloading(version, 0);
        Task::run(
            iced::stream::channel(8, async move |mut output| {
                let mut progress = output.clone();
                let result = update::download(&release, &dir, |percent| {
                    let _ = progress.try_send(Message::UpdateProgress(version, percent));
                })
                .await
                .map(|path| (version, path))
                .map_err(|e| e.to_string());
                let _ = output.send(Message::UpdateReady(result)).await;
            }),
            |message| message,
        )
    }

    /// Shows "Update X ready" now, or after the reply that is streaming.
    fn peek_update_if_pending(&mut self) -> Task<Message> {
        let visible = !matches!(self.presence.state(), State::Open | State::Hidden);
        if !self.update_peek_pending || self.request.is_some() || !visible {
            return Task::none();
        }
        self.update_peek_pending = false;
        match self.ready_text() {
            Some(text) => self.present(Event::Peek(text)),
            None => Task::none(),
        }
    }

    /// Saves, starts the installer (which reopens bloop) and quits; stays open on failure.
    fn install_update(&mut self) -> Task<Message> {
        let UpdateState::Ready(_, path) = &self.update_state else {
            return Task::none();
        };
        if self.request.is_some() {
            return Task::none();
        }
        if !self.idle_storage() {
            // Finish the queued writes first; `jobs` comes back here when they're done.
            self.install_after_save = true;
            return Task::none();
        }
        let path = path.clone();
        if let (Some(db), false) = (&self.db_path, self.storage_disabled)
            && let Err(error) = storage::save(db, &self.config, Some((&self.chat, &self.turns)))
        {
            self.update_state =
                UpdateState::Failed(format!("Couldn't save before updating: {error}"));
            return Task::none();
        }
        match (self.installer_launcher)(&path) {
            Ok(()) => {
                self.quitting = true;
                iced::exit()
            }
            Err(error) => {
                self.update_state =
                    UpdateState::Failed(format!("Couldn't start the update: {error}"));
                Task::none()
            }
        }
    }

    fn apply_live(&mut self, change: impl Fn(&mut Config)) -> Task<Message> {
        change(&mut self.config);
        change(&mut self.draft_config);
        self.persist()
    }

    /// After a notch size or position change: retarget the notch and re-place the window.
    fn relayout(&mut self, animate: bool) -> Task<Message> {
        self.retarget(self.presence.shape(), Instant::now());
        if !animate {
            self.spring.snap();
        }
        self.place_window(platform::Target::Current)
    }

    /// Points the spring at `shape`. Starting from rest, the clock starts now, so the first
    /// frame advances by its real interval instead of a guessed 1/60 s.
    fn retarget(&mut self, shape: Shape, now: Instant) {
        if !self.animating() {
            self.last_frame = Some(now);
        }
        let target = self.size_for(shape);
        self.spring.set_target(target.width, target.height);
        if self.reduce_motion() {
            self.spring.snap();
        }
    }

    fn place_window(&mut self, target: platform::Target) -> Task<Message> {
        self.placements += 1;
        let Some(id) = self.window else {
            return Task::none();
        };
        let canvas = self.canvas_size();
        let position = self.config.notch_position;
        self.region = None;
        Task::batch([
            window::run(id, move |w| platform::place_on(w, canvas, position, target)).discard(),
            self.update_region(),
        ])
    }

    /// Re-cuts the window region to the drawn outline when it moved by a whole pixel, so
    /// clicks outside the visible notch reach the windows below.
    fn update_region(&mut self) -> Task<Message> {
        let Some(id) = self.window else {
            return Task::none();
        };
        let rect = self.notch_rect();
        let key = (
            rect.width.round() as i32,
            rect.height.round() as i32,
            rect.x.round() as i32,
        );
        if self.region == Some(key) {
            return Task::none();
        }
        self.region = Some(key);
        let outline: Vec<(f32, f32)> =
            mascot::notch_outline(rect.size(), self.shoulder(), self.corner_radius())
                .into_iter()
                .map(|(x, y)| (x + rect.x, y))
                .collect();
        window::run(id, move |window| platform::shape(window, &outline)).discard()
    }

    /// With `BLOOP_FRAME_LOG=<file>`, appends one line per animation: frame count, the
    /// longest interval and how many frames took over 2.5 × the median.
    fn write_frame_log(&mut self) {
        let Some(path) = std::env::var_os("BLOOP_FRAME_LOG") else {
            return;
        };
        let mut dts = std::mem::take(&mut self.frame_log);
        if dts.len() < 2 {
            return;
        }
        dts.remove(0); // the first frame has no real interval
        let mut sorted = dts.clone();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        let longest = sorted[sorted.len() - 1];
        let slow = dts.iter().filter(|dt| **dt > median * 2.5).count();
        let line = format!(
            "frames={} median_ms={:.2} longest_ms={:.2} slow={}\n",
            dts.len() + 1,
            median * 1000.0,
            longest * 1000.0,
            slow
        );
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn corner_radius(&self) -> f32 {
        (self.size().height / 2.0).min(26.0)
    }

    /// Shoulders shrink with the notch so the 5 px sliver keeps its silhouette.
    fn shoulder(&self) -> f32 {
        (self.size().height * 0.6).clamp(3.0, SHOULDER)
    }

    fn scroll(&self) -> Task<Message> {
        widget::operation::snap_to("conversation", scrollable::RelativeOffset::END)
    }

    fn button<'a>(&self, label: &'a str, variant: Variant) -> button::Button<'a, Message> {
        button(text(label).size(13).font(MEDIUM))
            .padding([7, 12])
            .style(style::btn(self.tokens(), variant))
    }

    /// A ghost button showing `icon` with a tooltip, or `word` until the icon font loads.
    fn icon_button<'a>(
        &self,
        icon: Icon,
        word: &'a str,
        tip: &'a str,
        message: Message,
    ) -> Element<'a, Message> {
        self.icon_action(icon, word, tip, Variant::Ghost, Some(message))
    }

    /// A button showing `icon` with `tip` on hover, or `word` until the icon font loads.
    /// `None` shows it disabled.
    fn icon_action<'a>(
        &self,
        icon: Icon,
        word: &'a str,
        tip: &'a str,
        variant: Variant,
        message: Option<Message>,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        let (label, padding): (Element<'a, Message>, [u16; 2]) = if self.icons_ready {
            (
                text(icon.codepoint().to_string())
                    .font(icons::FONT)
                    .size(16)
                    .into(),
                [7, 9],
            )
        } else {
            (text(word).size(13).font(MEDIUM).into(), [7, 12])
        };
        let button = button(label)
            .padding(padding)
            .style(style::btn(t, variant))
            .on_press_maybe(message);
        self.tip(button, tip)
    }

    /// `content` with `tip` shown below it on hover.
    fn tip<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        tip: &'a str,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        widget::tooltip(
            content,
            container(text(tip).size(12))
                .padding([4, 8])
                .style(style::card(t, t.background)),
            widget::tooltip::Position::Bottom,
        )
        .into()
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
            Mood::Laughing => "Haha",
            Mood::Mad => "Hmph",
            Mood::Oops => "Oops",
            Mood::Sleepy => "Napping",
            Mood::Typing => "Listening…",
            Mood::Idle | Mood::Perk => "Ask anything",
        }
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message> {
        if self.app_window == Some(id) {
            return self.app_view();
        }
        self.notch_view()
    }

    fn notch_view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let shape = self.presence.shape();
        let target = self.size_for(shape);
        let rect = self.notch_rect();
        let content: Element<'_, Message> = match shape {
            Shape::Sliver => Space::new().into(),
            Shape::Expanded => self.panel_content(),
            Shape::Compact => self.pill_content(),
        };
        // Content is laid out at its final size and revealed as the notch grows into it:
        // it fades in over the last 40 % of the way and settles 6 px downward.
        let (now, goal) = (rect.height.max(1.0), target.height.max(1.0));
        let ratio = now.min(goal) / now.max(goal);
        let shown = ((ratio - 0.6) / 0.4).clamp(0.0, 1.0);
        let notch = |fill: iced::Color, outline: Option<iced::Color>| {
            canvas(Notch {
                fill,
                outline,
                shoulder: self.shoulder(),
                radius: self.corner_radius(),
            })
            .width(rect.width)
            .height(rect.height)
        };
        let target_rect = self.content_rect();
        let content = container(content)
            .width(target_rect.width)
            .height(target_rect.height)
            .padding(Padding {
                top: 6.0 * (1.0 - shown),
                ..Padding::ZERO
            });
        // Layers over the whole fixed canvas: the notch fill, the content at its final size,
        // then a veil in the notch's own shape that fades the content in, with the light
        // theme's outline drawn last so it stays crisp.
        let outline = self.light().then_some(t.border);
        let veil = iced::Color {
            a: 1.0 - shown,
            ..t.background
        };
        stack![
            Space::new().width(Length::Fill).height(Length::Fill),
            place(rect, notch(t.background, None).into()),
            place(target_rect, content.into()),
            place(rect, notch(veil, outline).into()),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The open panel: header, separator and chat or setup.
    fn panel_content(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let status = self.presence.message().unwrap_or(self.status_line());
        let poke = button(self.mascot(36.0))
            .padding(0)
            .style(style::btn(t, Variant::Bare))
            .on_press(Message::PokeMascot);
        let header = row![
            poke,
            column![
                text("bloop").size(15).font(MEDIUM),
                text(status).size(12).color(t.muted_foreground),
            ],
            Space::new().width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let (theme_icon, theme_word, theme_tip) = if self.light() {
            (Icon::Moon, "Dark", "Dark mode")
        } else {
            (Icon::Sun, "Light", "Light mode")
        };
        let header = header
            .push(self.icon_button(theme_icon, theme_word, theme_tip, Message::Theme))
            .push(self.icon_button(
                Icon::SlidersHorizontal,
                "Settings",
                "Settings",
                Message::OpenWindow,
            ))
            .push(self.icon_button(Icon::ChevronUp, "Hide", "Hide", Message::Toggle));
        let separator = container(Space::new())
            .width(Length::Fill)
            .height(1)
            .style(style::separator(t));
        column![header, separator, self.chat_view()]
            .spacing(14)
            .padding(Padding {
                top: 10.0,
                bottom: 18.0,
                left: SHOULDER + 18.0,
                right: SHOULDER + 18.0,
            })
            .into()
    }

    /// The pill shown on hover and for peeks.
    fn pill_content(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let status = self.pill_status();
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
    }

    /// Keeps one parsed reply per turn. A reply whose text grew since it was last parsed
    /// is extended rather than parsed again, so streaming only parses the new text.
    fn sync_replies(&mut self) {
        self.replies.truncate(self.turns.len());
        for index in 0..self.turns.len() {
            let shown = shown(&self.turns, self.reveal, index);
            match self.replies.get_mut(index) {
                Some(reply) => reply.update(shown),
                None => self.replies.push(rich::Reply::new(shown)),
            }
        }
    }

    /// The part of a reply on screen: all of it, except while the newest one types out.
    fn shown(&self, index: usize) -> &str {
        shown(&self.turns, self.reveal, index)
    }

    fn shown_last(&self) -> &str {
        self.shown(self.turns.len().saturating_sub(1))
    }

    /// Types out a few more characters of the newest reply: at least three per tick
    /// (about 90 a second), more when text piles up, so it never lags far behind. Once the
    /// reply is complete and fully shown, Bloop has its reaction.
    fn advance_reveal(&mut self) {
        if let Some(end) = self.reveal {
            let text = self.turns.last().map_or("", |t| t.assistant.as_str());
            let rest = text.get(end..).unwrap_or_default();
            let step = (rest.chars().count() / 6).max(3);
            let added = rest.char_indices().nth(step).map_or(rest.len(), |(i, _)| i);
            let end = end + added;
            if end < text.len() || self.request.is_some() {
                self.reveal = Some(end);
                self.sync_replies();
                return;
            }
            self.reveal = None;
            self.sync_replies();
        }
        if self.request.is_none()
            && let Some(mood) = self.after_reveal.take()
        {
            let seconds = if mood == Mood::Laughing { 2.4 } else { 1.6 };
            self.react(mood, seconds);
        }
    }

    /// Shows the newest reply in full at once and drops any pending reaction.
    fn finish_reveal(&mut self) {
        self.reveal = None;
        self.after_reveal = None;
        self.sync_replies();
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
            let greeter = button(self.mascot(72.0))
                .padding(0)
                .style(style::btn(t, Variant::Bare))
                .on_press(Message::PokeMascot);
            history = history
                .push(Space::new().height(8))
                .push(greeter)
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
            let mut label = row![].spacing(8).align_y(Alignment::Center);
            let (mood, since) = self.reply_mood(index);
            if self.config.mascot {
                let blink = index == last && self.blink_until.is_some_and(|u| self.now < u);
                label = label.push(self.draw_mascot(mood, since, blink, 22.0));
            }
            label = label.push(
                text("bloop")
                    .size(12)
                    .font(MEDIUM)
                    .color(t.muted_foreground),
            );
            let doing = match mood {
                Mood::Thinking => "is thinking…",
                Mood::Talking => "is typing…",
                Mood::Laughing if index == last => "is cracking up",
                Mood::Mad if index == last => "is grumpy",
                _ => "",
            };
            if !doing.is_empty() {
                label = label.push(text(doing).size(12).color(t.muted_foreground));
            }
            let streaming = index == last && self.request.is_some();
            if !turn.complete && !streaming {
                label = label.push(
                    container(text("Interrupted").size(11))
                        .padding([1, 8])
                        .style(style::badge(t)),
                );
            }
            let shown = self.shown(index);
            let reply: Option<Element<'_, Message>> = match self.replies.get(index) {
                // Bloop's face (or, without the mascot, "is thinking…") says enough.
                _ if shown.is_empty() => None,
                Some(reply) => Some(reply.view(t, self.copied.as_deref(), self.icons_ready)),
                None => Some(text(shown).size(14).into()),
            };
            let mut entry = column![label].spacing(4);
            if let Some(reply) = reply {
                entry = entry.push(reply);
            }
            if let Some(actions) = self.reply_actions(index, turn) {
                entry = entry.push(actions);
            }
            history = history
                .push(row![Space::new().width(Length::Fill), bubble])
                .push(entry);
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
        if let Some(card) = self.attachment_card() {
            body = body.push(card);
        }
        if self.editing {
            body = body.push(
                row![
                    text("Editing your last message. Sending replaces it and its reply.")
                        .size(12)
                        .color(t.muted_foreground),
                    Space::new().width(Length::Fill),
                    self.small_button(Icon::X, "Cancel", "Stop editing", Message::CancelEdit),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        let input = text_input("What's on your mind?", &self.input)
            .id("prompt")
            .padding([8, 6])
            .size(14)
            .on_input(Message::Input)
            .on_submit(Message::Send)
            .style(style::bare_input(t));
        let action = if self.request.is_some() {
            self.icon_action(
                Icon::Square,
                "Stop",
                "Stop the reply",
                Variant::Secondary,
                Some(Message::Cancel),
            )
        } else {
            self.icon_action(
                Icon::ArrowUp,
                "Send",
                "Send",
                Variant::Primary,
                (!self.loading && (!self.input.trim().is_empty() || self.attachment.is_some()))
                    .then_some(Message::Send),
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
        let idle = self.request.is_none() && !self.loading;
        let mut footer = row![self.model_switcher(), self.context_selector()]
            .spacing(2)
            .align_y(Alignment::Center);
        if self.attachment.is_none() {
            footer = footer.push(self.small_button(
                Icon::ClipboardPaste,
                "Paste clipboard",
                "Ask about the clipboard",
                Message::AskClipboard,
            ));
        }
        footer = footer.push(self.small_button(
            Icon::MessagesSquare,
            "All chats",
            "All chats",
            Message::ShowChats,
        ));
        if idle && !self.turns.is_empty() {
            footer = footer.push(self.small_button(
                Icon::MessageSquarePlus,
                "New chat",
                "New chat (Ctrl+N)",
                Message::NewChat,
            ));
        }
        // Word fallbacks can need another line at the 480 px panel width.
        body.push(footer.wrap()).into()
    }

    /// The chat's model under the prompt; a menu of profiles once any are saved.
    fn model_switcher(&self) -> Element<'_, Message> {
        let t = self.tokens();
        if self.config.profiles.is_empty() {
            return self.tip(
                container(
                    text(self.badge_text())
                        .size(13)
                        .wrapping(text::Wrapping::None),
                )
                .width(156)
                .clip(true)
                .padding([4, 10])
                .style(style::badge(t)),
                "Current model",
            );
        }
        let options = self.profile_options();
        let selected = options.iter().find(|o| o.id == self.chat.profile).cloned();
        container(
            pick_list(options, selected, |option| Message::PickProfile(option.id))
                .placeholder("Profile removed")
                .text_size(13)
                .padding([4, 10])
                .width(156)
                .style(style::select(t))
                .menu_style(style::menu(t)),
        )
        .clip(true)
        .into()
    }

    fn context_selector(&self) -> Element<'_, Message> {
        let t = self.tokens();
        self.tip(
            pick_list(
                ContextMode::ALL,
                Some(self.config.context),
                Message::SetContext,
            )
            .text_size(13)
            .padding([4, 10])
            .style(style::select(t))
            .menu_style(style::menu(t)),
            "How much of this chat is sent",
        )
    }

    /// The clipboard text waiting to go with the next prompt, and where it will go.
    fn attachment_card(&self) -> Option<Element<'_, Message>> {
        let t = self.tokens();
        let (text_value, cut) = self.attachment.as_ref()?;
        let size = if text_value.len() < 1024 {
            format!("{} bytes", text_value.len())
        } else {
            format!("{:.1} KB", text_value.len() as f32 / 1024.0)
        };
        let heading = if *cut {
            format!("Clipboard text · {size} (cut to fit)")
        } else {
            format!("Clipboard text · {size}")
        };
        let preview: String = text_value
            .lines()
            .take(3)
            .map(|line| model::clip_chars(line, 80))
            .collect::<Vec<_>>()
            .join("\n");
        Some(
            container(
                column![
                    row![
                        text(heading).size(12).font(MEDIUM),
                        Space::new().width(Length::Fill),
                        self.small_button(Icon::X, "Remove", "Remove", Message::RemoveAttachment),
                    ]
                    .align_y(Alignment::Center),
                    text(preview).size(12).color(t.muted_foreground),
                    text(format!("Sends with your question to {}", self.badge_text()))
                        .size(11)
                        .color(t.muted_foreground),
                ]
                .spacing(4),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(style::card(t, t.background))
            .into(),
        )
    }

    /// Copy under every finished reply; Retry and Edit under the last one too.
    /// Hidden while a reply streams or types out.
    fn reply_actions(&self, index: usize, turn: &Turn) -> Option<Element<'_, Message>> {
        let last = index + 1 == self.turns.len();
        if last && (self.request.is_some() || self.reveal.is_some()) {
            return None;
        }
        let mut actions = row![].spacing(2).align_y(Alignment::Center);
        if !turn.assistant.is_empty() {
            let copied = self.copied.as_deref() == Some(turn.assistant.as_str());
            actions = actions.push(if copied {
                self.small_button(
                    Icon::Check,
                    "Copied",
                    "Copied",
                    Message::CopyCode(turn.assistant.clone()),
                )
            } else {
                self.small_button(
                    Icon::Copy,
                    "Copy",
                    "Copy reply",
                    Message::CopyCode(turn.assistant.clone()),
                )
            });
        }
        if last && !self.loading {
            actions = actions
                .push(self.small_button(Icon::RotateCcw, "Retry", "Retry", Message::Retry))
                .push(self.small_button(
                    Icon::Pencil,
                    "Edit",
                    "Edit your message",
                    Message::EditLast,
                ));
        }
        Some(actions.into())
    }

    /// A quiet icon button for actions under replies and the prompt, `word` until the icon
    /// font loads, with `tip` on hover.
    fn small_button<'a>(
        &self,
        icon: Icon,
        word: &'a str,
        tip: &'a str,
        message: Message,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        let label = if self.icons_ready {
            text(icon.codepoint().to_string())
                .font(icons::FONT)
                .size(13)
        } else {
            text(word).size(12)
        };
        let button = button(label.color(t.muted_foreground))
            .padding(if self.icons_ready { [3, 6] } else { [2, 8] })
            .style(style::btn(t, Variant::Ghost))
            .on_press(message);
        self.tip(button, tip)
    }

    /// Provider, endpoint and model fields, shared by Settings and setup.
    fn model_fields(&self) -> widget::Column<'_, Message> {
        let t = self.tokens();
        let mut fields = column![field(
            "Provider",
            pick_list(
                Provider::ALL,
                Some(self.draft_config.provider),
                Message::Provider,
            )
            .padding([10, 14])
            .text_size(15)
            .width(Length::Fill)
            .style(style::select(t))
            .menu_style(style::menu(t)),
        ),]
        .spacing(16);
        if self.draft_config.provider == Provider::ChatGpt {
            return fields.push(self.chatgpt_fields());
        }
        if !self.draft_config.provider.is_cli() {
            fields = fields.push(field(
                "Endpoint base URL",
                self.input(
                    self.draft_config.provider.default_endpoint(),
                    &self.draft_config.endpoint,
                    Message::Endpoint,
                ),
            ));
        }
        fields = fields.push(field(
            "Model",
            self.input(
                if self.draft_config.provider.is_cli() {
                    "Leave blank for the CLI's default"
                } else {
                    "qwen3:4b"
                },
                &self.draft_config.model,
                Message::Model,
            ),
        ));
        if let Some(key) = self.key_field() {
            fields = fields.push(key);
        }
        fields
    }

    fn chatgpt_fields(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let enabled = self.chatgpt_can_manage();
        let summary = if self.chatgpt.signed_in {
            let account = self.chatgpt.account.as_deref().unwrap_or("ChatGPT account");
            if self.chatgpt.plan_enabled {
                format!("Using ChatGPT plan · {account}")
            } else {
                format!("Signed in as {account}. ChatGPT plan usage is off.")
            }
        } else {
            "Use your ChatGPT plan in Bloop.".into()
        };
        let mut actions = row![].spacing(8);
        if self.chatgpt_cancel.is_some() {
            actions = actions.push(self.icon_action(
                Icon::X,
                "Cancel",
                "Cancel ChatGPT sign-in",
                Variant::Outline,
                Some(Message::ChatGptCancel),
            ));
        } else if self.chatgpt.signed_in {
            let label = if self.chatgpt.plan_enabled {
                "Reconnect"
            } else {
                "Enable ChatGPT plan usage"
            };
            actions = actions
                .push(
                    self.button(label, Variant::Outline)
                        .on_press_maybe(enabled.then_some(Message::ChatGptSignIn(false))),
                )
                .push(
                    self.button("Sign out", Variant::Outline)
                        .on_press_maybe(enabled.then_some(Message::ChatGptSignOut)),
                );
        } else {
            actions = actions.push(
                self.button("Continue with ChatGPT", Variant::Primary)
                    .on_press_maybe(
                        (enabled && cfg!(windows)).then_some(Message::ChatGptSignIn(false)),
                    ),
            );
            if self.chatgpt.registered {
                actions = actions.push(
                    self.button("Use another account", Variant::Outline)
                        .on_press_maybe(enabled.then_some(Message::ChatGptSignIn(true))),
                );
            }
        }
        if self.chatgpt.registered {
            actions = actions.push(
                self.button("Manage usage", Variant::Ghost)
                    .on_press(Message::OpenLink(chatgpt::USAGE_URL.into())),
            );
        }
        let mut fields = column![
            text(summary).size(14),
            text("ChatGPT receives your prompt and recent chat context.")
                .size(12)
                .color(t.muted_foreground),
            actions.wrap(),
        ]
        .spacing(10);
        if !self.chatgpt_notice.is_empty() {
            fields = fields.push(text(&self.chatgpt_notice).size(13));
        }
        if self.chatgpt.plan_enabled {
            let slug = self.draft_config.model.trim();
            let mut models = vec![ChatGptModel {
                slug: String::new(),
                display_name: "Automatic".into(),
            }];
            models.extend(self.chatgpt_models.iter().cloned());
            if !models.iter().any(|model| model.slug == slug) {
                models.push(ChatGptModel {
                    slug: slug.into(),
                    display_name: slug.into(),
                });
            }
            let selected = models.iter().find(|model| model.slug == slug).cloned();
            fields = fields.push(field(
                "Model",
                row![
                    pick_list(models, selected, |model| Message::Model(model.slug))
                        .padding([10, 14])
                        .text_size(15)
                        .width(Length::Fill)
                        .style(style::select(t))
                        .menu_style(style::menu(t)),
                    self.icon_action(
                        Icon::RefreshCw,
                        "Refresh models",
                        "Refresh the model list",
                        Variant::Outline,
                        enabled.then_some(Message::ChatGptRefreshModels),
                    ),
                ]
                .align_y(Alignment::Center)
                .spacing(8),
            ));
        }
        fields.into()
    }
}

/// `inner` placed `rect.x` from the left of the canvas, top-aligned.
fn place(rect: iced::Rectangle, inner: Element<'_, Message>) -> Element<'_, Message> {
    column![row![Space::new().width(rect.x), inner]]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn field<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(13).font(MEDIUM), control.into()]
        .spacing(6)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bloop::config::{Accent, NotchPosition, NotchWidth, ReduceMotion};
    use bloop::launch::Launch;
    use bloop::motion::Feel;
    use bloop::onboarding::{Choice, Step};
    use bloop::update::{Release, Version};

    fn ready() -> Island {
        let (mut app, _) = Island::new(Launch::Manual);
        app.window = Some(window::Id::unique());
        app.loading = false;
        app.storage_disabled = true;
        app
    }

    #[test]
    fn context_changes_apply_and_queue_a_save_without_saving_model_edits() {
        let mut app = ready();
        app.storage_disabled = false;
        app.db_path = Some(PathBuf::from("unused-context-test.sqlite3"));
        app.saving = true;
        app.draft_config.model = "unsaved-model".into();
        for mode in ContextMode::ALL {
            let _ = app.update(Message::SetContext(mode));
            assert_eq!(app.config.context, mode);
            assert_eq!(app.draft_config.context, mode);
            assert_eq!(app.draft_config.model, "unsaved-model");
            assert_eq!(app.settings_to_save().unwrap().context, mode);
            assert!(
                matches!(app.jobs.back(), Some(jobs::Job::Save { config, .. })
                if config.context == mode && config.model != "unsaved-model")
            );
        }
    }

    #[test]
    fn pasted_key_fills_only_the_matching_server_and_profile() {
        let mut app = ready();
        app.draft_config.provider = Provider::OpenAi;
        app.key_status = KeySource::None;
        let endpoint = app.draft_config.endpoint.clone();
        let pasted = |provider, endpoint, profile, value: Option<&str>| {
            Message::KeyPasted(provider, endpoint, profile, value.map(str::to_owned))
        };
        let _ = app.update(pasted(
            Provider::OpenAi,
            endpoint.clone(),
            ProfileEdit::Default,
            Some(" sk-test\r\n"),
        ));
        assert_eq!(app.key_draft, "sk-test");
        for message in [
            pasted(
                Provider::Unsloth,
                endpoint.clone(),
                ProfileEdit::Default,
                Some("wrong-provider"),
            ),
            pasted(
                Provider::OpenAi,
                "https://other.example".into(),
                ProfileEdit::Default,
                Some("wrong-server"),
            ),
            pasted(
                Provider::OpenAi,
                endpoint.clone(),
                ProfileEdit::Existing(1),
                Some("wrong-profile"),
            ),
            pasted(
                Provider::OpenAi,
                endpoint.clone(),
                ProfileEdit::Default,
                None,
            ),
        ] {
            let _ = app.update(message);
            assert_eq!(app.key_draft, "sk-test");
        }
        app.key_status = KeySource::Environment;
        let _ = app.update(pasted(
            Provider::OpenAi,
            endpoint,
            ProfileEdit::Default,
            Some("ignored"),
        ));
        assert_eq!(app.key_draft, "sk-test");
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

    /// Sends `prompt` and finishes its reply with `reply`.
    fn exchange(app: &mut Island, prompt: &str, reply: &str) {
        app.input = prompt.into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        let _ = app.update(Message::Chunk(id, StreamEvent::Delta(reply.into())));
        let _ = app.update(Message::Finished(id, Ok(())));
        app.finish_reveal();
    }

    #[test]
    fn retry_replaces_the_last_turn() {
        let mut app = ready();
        exchange(&mut app, "first", "one");
        exchange(&mut app, "second", "bad");
        let _task = app.update(Message::Retry);
        assert_eq!(app.turns.len(), 2);
        assert_eq!(app.turns[1].user, "second");
        assert!(app.turns[1].assistant.is_empty());
        assert!(app.request.is_some());
        assert_eq!(app.replies.len(), app.turns.len());
    }

    #[test]
    fn edit_and_resend_replaces_the_last_turn_and_cancel_keeps_it() {
        let mut app = ready();
        exchange(&mut app, "first", "one");
        exchange(&mut app, "secnd", "huh?");
        let _ = app.update(Message::EditLast);
        assert!(app.editing);
        assert_eq!(app.input, "secnd");
        let _ = app.update(Message::CancelEdit);
        assert!(!app.editing && app.input.is_empty());
        assert_eq!(app.turns[1].assistant, "huh?");

        let _ = app.update(Message::EditLast);
        app.input = "second".into();
        let _task = app.update(Message::Send);
        assert!(!app.editing);
        assert_eq!(app.turns.len(), 2);
        assert_eq!(app.turns[1].user, "second");
    }

    #[test]
    fn retry_and_edit_are_ignored_while_streaming() {
        let mut app = ready();
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let _ = app.update(Message::Retry);
        let _ = app.update(Message::EditLast);
        assert_eq!(app.turns.len(), 1);
        assert!(!app.editing);
    }

    /// Feeds frames 1/60 s apart starting at `from`; returns the time after the last one.
    fn run_frames(app: &mut Island, from: Instant, count: u32) -> Instant {
        let mut at = from;
        for _ in 0..count {
            let _ = app.update(Message::Frame(at));
            at += Duration::from_micros(16_667);
        }
        at
    }

    #[test]
    fn animation_can_reverse_and_stops_at_rest() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        assert_eq!(app.size(), app.size_for(Shape::Sliver));
        let _ = app.present(Event::Shortcut);
        assert!(app.animating());
        let t = run_frames(&mut app, Instant::now(), 6);
        let panel = app.size_for(Shape::Expanded);
        let mid = app.size();
        assert!(mid.height > 5.0 && mid.height < panel.height, "{mid:?}");
        let _ = app.present(Event::Shortcut);
        // Momentum carries on for a moment: no jump back.
        let t = run_frames(&mut app, t, 1);
        assert!(app.size().height >= mid.height - 0.5, "{:?}", app.size());
        run_frames(&mut app, t, 180);
        assert_eq!(app.size(), app.size_for(Shape::Sliver));
        assert!(!app.animating());
    }

    #[test]
    fn window_is_always_the_widest_canvas() {
        let mut app = ready();
        for width in NotchWidth::ALL {
            app.config.notch_width = width;
            assert_eq!(app.canvas_size(), Size::new(600.0, 580.0), "{width:?}");
        }
    }

    #[test]
    fn shrinking_width_while_open_keeps_the_notch_inside_the_canvas() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        app.config.notch_width = NotchWidth::L;
        let _ = app.present(Event::Shortcut);
        run_frames(&mut app, Instant::now(), 120);
        let _ = app.update(Message::SetNotchWidth(NotchWidth::S));
        let mut at = Instant::now();
        for _ in 0..60 {
            let rect = app.notch_rect();
            assert!(
                rect.x >= 0.0 && rect.x + rect.width <= app.canvas_size().width + 0.01,
                "{rect:?}"
            );
            at = run_frames(&mut app, at, 1);
        }
    }

    #[test]
    fn content_is_laid_out_at_the_target_size_while_growing() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        let _ = app.present(Event::Shortcut);
        run_frames(&mut app, Instant::now(), 3);
        assert!(app.size().height < 300.0);
        let content = app.content_rect();
        assert_eq!(content.size(), app.size_for(Shape::Expanded));
        assert_eq!(content.x, (app.canvas_size().width - content.width) / 2.0);
    }

    #[test]
    fn first_frame_from_rest_is_a_real_frame_step() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        let start = Instant::now();
        let _ = app.present(Event::Shortcut);
        let _ = app.update(Message::Frame(start + Duration::from_micros(2_800)));
        let grown = app.size().height - 5.0;
        assert!(grown < 5.0, "first 360 Hz frame jumped {grown} px");
    }

    #[test]
    fn showing_the_notch_re_places_the_window() {
        let mut app = ready();
        let before = app.placements;
        let _ = app.present(Event::Shortcut);
        assert!(
            app.placements > before,
            "opening must re-place for display changes"
        );
    }

    #[test]
    fn reduce_motion_snaps() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::On;
        let _ = app.present(Event::Shortcut);
        assert_eq!(app.size(), app.size_for(Shape::Expanded));
        assert!(!app.animating());
    }

    #[test]
    fn no_frames_when_settled() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        assert!(!app.animating());
        let _ = app.present(Event::Shortcut);
        run_frames(&mut app, Instant::now(), 120);
        assert!(!app.animating());
        assert_eq!(app.size(), app.size_for(Shape::Expanded));
    }

    #[test]
    fn notch_rect_follows_position() {
        let mut app = ready();
        let canvas = app.canvas_size();
        let width = app.size().width;
        app.config.notch_position = NotchPosition::Centre;
        assert_eq!(app.notch_rect().x, (canvas.width - width) / 2.0);
        app.config.notch_position = NotchPosition::Left;
        assert_eq!(app.notch_rect().x, 0.0);
        app.config.notch_position = NotchPosition::Right;
        assert_eq!(app.notch_rect().x, canvas.width - width);
        assert_eq!(app.notch_rect().y, 0.0);
        assert_eq!(canvas, Size::new(600.0, 580.0));
    }

    #[test]
    fn peek_message_shows_in_compact() {
        let mut app = ready();
        let _ = app.update(Message::PeekTest);
        assert_eq!(app.presence.state(), State::Peek);
        assert!(app.presence.message().is_some());
        let (w, h) = (app.spring.w.target, app.spring.h.target);
        assert_eq!(Size::new(w, h), app.size_for(Shape::Compact));
    }

    #[test]
    fn up_and_down_recall_this_chats_prompts() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        let id = app.window.unwrap();
        for prompt in ["first", "second"] {
            app.turns.push(Turn {
                user: prompt.into(),
                assistant: "ok".into(),
                complete: true,
            });
        }
        let _ = app.update(Message::Recall(id, true));
        assert_eq!(app.input, "second");
        let _ = app.update(Message::Recall(id, true));
        assert_eq!(app.input, "first");
        // The oldest prompt stays put.
        let _ = app.update(Message::Recall(id, true));
        assert_eq!(app.input, "first");
        let _ = app.update(Message::Recall(id, false));
        assert_eq!(app.input, "second");
        let _ = app.update(Message::Recall(id, false));
        assert_eq!(app.input, "");
        // Down on an empty prompt and Up on a typed one do nothing.
        let _ = app.update(Message::Recall(id, false));
        assert_eq!(app.input, "");
        let _ = app.update(Message::Input("draft".into()));
        let _ = app.update(Message::Recall(id, true));
        assert_eq!(app.input, "draft");
    }

    #[test]
    fn ctrl_n_starts_a_new_chat_only_in_the_open_notch() {
        let mut app = ready();
        let notch = app.window.unwrap();
        app.turns.push(Turn {
            user: "hi".into(),
            assistant: "ok".into(),
            complete: true,
        });
        // Closed notch: nothing happens.
        let _ = app.update(Message::NewChatKey(notch));
        assert_eq!(app.turns.len(), 1);
        let _ = app.update(Message::Toggle);
        // Another window (the app window): nothing happens.
        let _ = app.update(Message::NewChatKey(window::Id::unique()));
        assert_eq!(app.turns.len(), 1);
        let _ = app.update(Message::NewChatKey(notch));
        assert!(app.turns.is_empty());
    }

    #[test]
    fn dressing_bloop_applies_at_once_and_poking_makes_it_giggle() {
        let mut app = ready();
        let look = Look {
            hat: bloop::config::Hat::TopHat,
            ..Look::default()
        };
        let _ = app.update(Message::SetLook(look));
        assert_eq!(app.config.look, look);
        assert_eq!(app.draft_config.look, look);
        let _ = app.update(Message::ShuffleLook);
        assert_ne!(app.config.look, look);
        let _ = app.update(Message::PokeMascot);
        assert_eq!(app.mood(), Mood::Laughing);
    }

    #[test]
    fn send_marks_presence_busy() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        assert_eq!(app.presence.state(), State::Open);
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let _ = app.update(Message::Focus(app.window.unwrap(), false));
        assert_eq!(app.presence.state(), State::Peek);
        let id = app.request_id;
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(app.presence.message(), Some("All done"));
    }

    #[test]
    fn cancelled_reply_does_not_pin_the_pill() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        app.cancel();
        let _ = app.update(Message::Focus(app.window.unwrap(), false));
        assert_eq!(app.presence.state(), State::Sliver);
    }

    #[test]
    fn hover_perks_mascot() {
        let mut app = ready();
        let _ = app.update(Message::Cursor(app.window.unwrap(), true));
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
        let _ = app.update(Message::JobDone(jobs::Done::Saved(Err("disk full".into()))));
        assert!(!app.quitting);
        assert_eq!(app.presence.state(), State::Open);
        assert!(app.storage_warning.contains("disk full"));
    }

    #[test]
    fn streamed_reply_types_out_and_reacts_after() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        app.input = "tell me a joke".into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        assert_eq!(app.reply_mood(0).0, Mood::Thinking);
        let burst = "Why did the scarecrow win an award? He was outstanding in his field.";
        let _ = app.update(Message::Chunk(id, StreamEvent::Delta(burst.into())));
        // A burst does not land all at once: only the first few characters show.
        let first = app.shown(0).len();
        assert!(first > 0 && first < burst.len(), "{first}");
        assert_eq!(app.reply_mood(0).0, Mood::Talking);
        let _ = app.update(Message::Finished(id, Ok(())));
        // Still typing out, so no reaction yet.
        assert_eq!(app.mood(), Mood::Talking);
        let mut ticks = 0;
        while app.reveal.is_some() {
            let _ = app.update(Message::Tick(Instant::now()));
            ticks += 1;
            assert!(
                ticks < 60,
                "reveal should catch up within about two seconds"
            );
        }
        assert!(ticks > 3, "reveal should take a few frames, took {ticks}");
        assert_eq!(app.shown(0), burst);
        assert_eq!(app.mood(), Mood::Laughing);
        assert_eq!(app.reply_mood(0).0, Mood::Laughing);
    }

    #[test]
    fn reduced_motion_shows_streamed_text_at_once() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::On;
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        let _ = app.update(Message::Chunk(
            id,
            StreamEvent::Delta("Hello there, friend.".into()),
        ));
        assert_eq!(app.shown(0), "Hello there, friend.");
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(app.mood(), Mood::Happy);
    }

    #[test]
    fn failed_or_stopped_reply_makes_bloop_mad_and_older_replies_sleep() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::Off;
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let id = app.request_id;
        let _ = app.update(Message::Chunk(
            id,
            StreamEvent::Delta("partial reply".into()),
        ));
        let _ = app.update(Message::Finished(id, Err("boom".into())));
        assert_eq!(app.shown(0), "partial reply");
        assert_eq!(app.mood(), Mood::Mad);
        app.input = "again".into();
        let _task = app.update(Message::Send);
        app.cancel();
        assert_eq!(app.reply_mood(0).0, Mood::Sleepy);
        assert_eq!(app.reply_mood(1).0, Mood::Mad);
        let _ = app.update(Message::Tick(Instant::now() + NAP_AFTER * 2));
        assert_eq!(app.reply_mood(1).0, Mood::Sleepy);
    }

    #[test]
    fn funny_replies_make_bloop_laugh() {
        assert_eq!(
            Mood::for_reply("hi", "Haha, nice one!", true),
            Mood::Laughing
        );
        assert_eq!(Mood::for_reply("hi", "That is 😂", true), Mood::Laughing);
        assert_eq!(
            Mood::for_reply("Tell me a joke", "Okay.", true),
            Mood::Laughing
        );
        assert_eq!(
            Mood::for_reply("fix punctuation", "Done.", true),
            Mood::Happy
        );
        assert_eq!(Mood::for_reply("hi", "A lollipop.", true), Mood::Happy);
        assert_eq!(Mood::for_reply("joke", "Haha", false), Mood::Mad);
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
        let (mut app, _) = Island::new(Launch::Manual);
        app.input = "hello".into();
        let _ = app.update(Message::Send);
        assert!(app.turns.is_empty());
    }

    fn loaded(config: Config) -> Message {
        Message::Loaded(Ok(storage::Loaded {
            config,
            ..Default::default()
        }))
    }

    #[test]
    fn new_chat_keeps_the_old_one_in_the_list() {
        let mut app = ready();
        exchange(&mut app, "keep me", "saved");
        let old = app.chat.id;
        assert_eq!(app.chats[0].id, old);
        assert_eq!(app.chats[0].title, "keep me");
        let _ = app.update(Message::NewChat);
        assert!(app.turns.is_empty());
        assert_ne!(app.chat.id, old);
        assert_eq!(app.chats.len(), 1, "an empty chat isn't listed");
        exchange(&mut app, "second", "ok");
        assert_eq!(app.chats.len(), 2);
        assert_eq!(app.chats[0].id, app.chat.id, "newest first");
    }

    fn with_cloud_profile(app: &mut Island) -> u32 {
        let id = app.config.next_profile_id();
        app.config.profiles.push(bloop::config::Profile {
            id,
            name: "Cloud".into(),
            provider: Provider::OpenAi,
            endpoint: "https://api.example.com/v1".into(),
            model: "big".into(),
        });
        id
    }

    #[test]
    fn switching_profile_with_history_starts_a_new_chat() {
        let mut app = ready();
        let cloud = with_cloud_profile(&mut app);
        let _ = app.update(Message::PickProfile(cloud));
        assert_eq!(app.chat.profile, cloud, "an empty chat just switches");
        let _ = app.update(Message::PickProfile(0));
        exchange(&mut app, "local secret", "ok");
        assert_eq!(app.chat.destination, "local");
        let local_chat = app.chat.id;
        let _ = app.update(Message::PickProfile(cloud));
        assert_ne!(app.chat.id, local_chat);
        assert!(app.turns.is_empty());
        assert_eq!(app.chat.profile, cloud);
    }

    #[test]
    fn local_history_is_never_sent_to_a_changed_cloud_default() {
        let mut app = ready();
        exchange(&mut app, "local secret", "ok");
        app.config.provider = Provider::OpenAi;
        app.config.endpoint = "https://api.example.com/v1".into();
        app.input = "next".into();
        let _ = app.update(Message::Send);
        assert!(app.request.is_none());
        assert_eq!(app.turns.len(), 1);
        assert!(app.status.contains("api.example.com"), "{}", app.status);
        // Retry would resend the local prompt to the cloud too.
        let _ = app.update(Message::Retry);
        assert!(app.request.is_none());
        // A new chat may use it.
        let _ = app.update(Message::NewChat);
        app.input = "fresh".into();
        let _task = app.update(Message::Send);
        assert!(app.request.is_some());
        assert_eq!(app.chat.destination, "cloud:https://api.example.com");
    }

    #[test]
    fn deleted_profile_blocks_sending_until_a_new_chat() {
        let mut app = ready();
        let cloud = with_cloud_profile(&mut app);
        let _ = app.update(Message::PickProfile(cloud));
        app.config.profiles.clear();
        app.input = "hi".into();
        let _ = app.update(Message::Send);
        assert!(app.request.is_none());
        assert!(app.status.contains("deleted"));
        let _ = app.update(Message::NewChat);
        assert_eq!(app.chat.profile, 0);
    }

    #[test]
    fn clipboard_text_goes_with_the_next_prompt_once() {
        let mut app = ready();
        let _ = app.update(Message::Clipboard(app.chat.id, Some("   ".into())));
        assert!(app.attachment.is_none());
        assert!(!app.status.is_empty());
        let _ = app.update(Message::Clipboard(
            app.chat.id,
            Some("Error 0x80070005".into()),
        ));
        assert!(app.attachment.is_some());
        let _task = app.update(Message::Send);
        assert!(app.request.is_some(), "an attachment alone can be sent");
        assert!(app.turns[0].user.starts_with("Explain this."));
        assert!(app.turns[0].user.contains("Error 0x80070005"));
        assert!(app.attachment.is_none());
        assert_eq!(app.chat.title, "Explain this.");
    }

    #[test]
    fn too_long_question_with_clipboard_is_refused() {
        let mut app = ready();
        let _ = app.update(Message::Clipboard(app.chat.id, Some("x".repeat(6000))));
        app.input = "q".repeat(3000);
        let _ = app.update(Message::Send);
        assert!(app.request.is_none());
        assert!(
            app.attachment.is_some(),
            "kept so the user can shorten the question"
        );
    }

    fn turns(user: &str) -> Vec<Turn> {
        vec![Turn {
            user: user.into(),
            assistant: "ok".into(),
            complete: true,
        }]
    }

    #[test]
    fn switching_chats_keeps_the_outgoing_chats_save_in_order() {
        let mut app = ready();
        app.storage_disabled = false;
        app.db_path = Some(PathBuf::from("unused.sqlite3"));
        app.saving = true; // a save is running
        exchange(&mut app, "chat A", "final reply");
        let a = app.chat.id;
        let _ = app.update(Message::NewChat);
        let _ = app.update(Message::Chat(ChatAction::Open(42)));
        // A's finished turns are queued, and the load of chat 42 comes after them.
        assert!(
            matches!(&app.jobs[0], jobs::Job::Save { chat: Some((c, t)), .. }
            if c.id == a && t[0].assistant == "final reply")
        );
        assert!(matches!(app.jobs.back(), Some(jobs::Job::Load(42))));
    }

    #[test]
    fn deleting_a_chat_drops_its_queued_saves_and_waits_for_the_running_one() {
        let mut app = ready();
        app.storage_disabled = false;
        app.db_path = Some(PathBuf::from("unused.sqlite3"));
        app.saving = true;
        exchange(&mut app, "chat A", "reply");
        let a = app.chat.id;
        let _ = app.update(Message::Chat(ChatAction::Delete(a)));
        let _ = app.update(Message::Chat(ChatAction::DeleteConfirm));
        assert_ne!(app.chat.id, a, "the notch moved to a new chat");
        assert!(app.chats.iter().all(|c| c.id != a));
        assert!(app.jobs.iter().all(|job| !matches!(job,
            jobs::Job::Save { chat: Some((c, _)), .. } if c.id == a)));
        assert!(matches!(app.jobs.back(), Some(jobs::Job::Delete(id)) if *id == a));
    }

    #[test]
    fn quit_waits_for_queued_jobs() {
        let mut app = ready();
        app.storage_disabled = false;
        app.db_path = Some(PathBuf::from("unused.sqlite3"));
        app.saving = true;
        let _ = app.update(Message::Quit);
        assert!(app.quitting);
        assert!(!app.jobs.is_empty(), "the final save waits its turn");
    }

    #[test]
    fn new_chat_and_late_clipboard_reads_drop_the_attachment() {
        let mut app = ready();
        let old = app.chat.id;
        let _ = app.update(Message::Clipboard(old, Some("private".into())));
        exchange(&mut app, "q", "a");
        let _ = app.update(Message::Clipboard(app.chat.id, Some("private".into())));
        let _ = app.update(Message::NewChat);
        assert!(app.attachment.is_none());
        let _ = app.update(Message::Clipboard(old, Some("late".into())));
        assert!(
            app.attachment.is_none(),
            "a read for another chat is ignored"
        );
    }

    #[test]
    fn chats_from_an_older_bloop_are_never_sent() {
        let mut app = ready();
        app.chat.destination = model::UNKNOWN_DESTINATION.into();
        app.turns = turns("old private prompt");
        app.sync_replies();
        app.input = "more".into();
        let _ = app.update(Message::Send);
        let _ = app.update(Message::Retry);
        assert!(app.request.is_none());
        assert!(app.status.contains("older bloop"));
    }

    #[test]
    fn a_pasted_key_never_follows_a_server_change() {
        let mut app = ready();
        app.draft_config.provider = Provider::OpenAi;
        app.draft_config.endpoint = "https://a.example.com/v1".into();
        let _ = app.update(Message::ApiKey("sk-a".into()));
        let _ = app.update(Message::Endpoint("https://a.example.com/v2".into()));
        assert_eq!(app.key_draft, "sk-a", "same server keeps it");
        let _ = app.update(Message::Endpoint("https://b.example.com/v1".into()));
        assert!(app.key_draft.is_empty());
        let _ = app.update(Message::ApiKey("sk-a".into()));
        let _ = app.update(Message::EditProfile(0));
        assert!(app.key_draft.is_empty());
    }

    #[test]
    fn a_pasted_key_never_follows_a_picked_server() {
        let mut app = ready();
        let _ = app.update(Message::PickChoice(Choice::ollama(
            "http://127.0.0.1:11434",
            "a",
        )));
        let _ = app.update(Message::ApiKey("sk-a".into()));
        let _ = app.update(Message::PickChoice(Choice::ollama(
            "http://127.0.0.1:8080",
            "b",
        )));
        assert!(app.key_draft.is_empty());
        let _ = app.update(Message::ApiKey("sk-a".into()));
        let _ = app.update(Message::NewProfile);
        assert!(app.key_draft.is_empty());
    }

    fn queued_storage() -> Island {
        let mut app = ready();
        app.storage_disabled = false;
        app.db_path = Some(PathBuf::from("unused.sqlite3"));
        app.saving = true; // a job is running
        app
    }

    fn other_chat(id: i64) -> (Chat, Vec<Turn>) {
        let chat = Chat {
            id,
            title: "Other".into(),
            ..Chat::default()
        };
        (chat, turns("other"))
    }

    #[test]
    fn a_deleted_chat_is_never_opened() {
        let mut app = queued_storage();
        let _ = app.update(Message::Chat(ChatAction::Open(42)));
        let _ = app.update(Message::Chat(ChatAction::Delete(42)));
        let _ = app.update(Message::Chat(ChatAction::DeleteConfirm));
        assert!(
            app.jobs
                .iter()
                .all(|job| !matches!(job, jobs::Job::Load(42)))
        );
        // A read already under way lands after the delete was asked for.
        let _ = app.update(Message::JobDone(jobs::Done::Loaded(42, Ok(other_chat(42)))));
        assert_ne!(app.chat.id, 42);
    }

    #[test]
    fn a_late_chat_read_never_hides_a_new_reply() {
        let mut app = queued_storage();
        let _ = app.update(Message::Chat(ChatAction::Open(42)));
        exchange(&mut app, "here", "reply");
        let current = app.chat.id;
        let _ = app.update(Message::JobDone(jobs::Done::Loaded(42, Ok(other_chat(42)))));
        assert_eq!(app.chat.id, current);
        assert_eq!(app.turns[0].assistant, "reply");
    }

    #[test]
    fn opening_a_chat_shows_it_when_nothing_changed() {
        let mut app = queued_storage();
        let _ = app.update(Message::Chat(ChatAction::Open(42)));
        let _ = app.update(Message::JobDone(jobs::Done::Loaded(42, Ok(other_chat(42)))));
        assert_eq!(app.chat.id, 42);
        assert_eq!(app.turns[0].user, "other");
    }

    #[test]
    fn deleting_a_chat_keeps_queued_settings() {
        let mut app = queued_storage();
        exchange(&mut app, "chat A", "reply");
        let a = app.chat.id;
        app.config.ask_shortcut = "Ctrl+Alt+Q".into();
        let _ = app.persist();
        let _ = app.update(Message::Chat(ChatAction::Delete(a)));
        let _ = app.update(Message::Chat(ChatAction::DeleteConfirm));
        assert!(app.jobs.iter().any(|job| matches!(job,
            jobs::Job::Save { config, chat: None } if config.ask_shortcut == "Ctrl+Alt+Q")));
    }

    #[test]
    fn exporting_another_chat_waits_for_its_saves() {
        let mut app = queued_storage();
        let _ = app.update(Message::Chat(ChatAction::Export(42)));
        assert!(matches!(app.jobs.back(), Some(jobs::Job::Export(42))));
    }

    #[test]
    fn a_deferred_update_survives_a_reply() {
        let mut app = queued_storage();
        app.install_after_save = true;
        app.input = "hi".into();
        let _task = app.update(Message::Send);
        app.jobs.clear();
        let _ = app.update(Message::JobDone(jobs::Done::Saved(Ok(()))));
        assert!(app.install_after_save, "still waiting for the reply");
        let id = app.request_id;
        let _ = app.update(Message::Finished(id, Ok(())));
        assert!(
            !app.idle_storage(),
            "the save after the reply is under way and drains to the update"
        );
        let _ = app.update(Message::JobDone(jobs::Done::Saved(Ok(()))));
        assert!(!app.install_after_save, "the update was attempted");
    }

    #[test]
    fn custom_instructions_save_globally_and_trim() {
        let mut app = ready();
        let cloud = with_cloud_profile(&mut app);
        let _ = app.update(Message::Instructions(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("  I write Rust.\nBe brief.  ".into())),
        )));
        let _ = app.update(Message::EditProfile(cloud));
        assert!(app.config.instructions.is_empty(), "edits wait for Save");
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.config.instructions, "I write Rust.\nBe brief.");
        assert_eq!(
            app.config.with_profile(0).unwrap().instructions,
            app.config.with_profile(cloud).unwrap().instructions
        );
        assert_eq!(app.draft_config.instructions, app.config.instructions);
        assert_eq!(app.instructions_editor.text(), app.config.instructions);
        let _ = app.update(Message::Instructions(text_editor::Action::SelectAll));
        let _ = app.update(Message::Instructions(text_editor::Action::Edit(
            text_editor::Edit::Backspace,
        )));
        let _ = app.update(Message::SaveSettings);
        assert!(
            app.config.instructions.is_empty(),
            "instructions can be cleared"
        );
    }

    #[test]
    fn custom_instructions_over_limit_leave_saved_settings_alone() {
        let mut app = ready();
        let _ = app.update(Message::Instructions(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("é".repeat(1025))),
        )));
        let _ = app.update(Message::SaveSettings);
        assert!(app.config.instructions.is_empty());
        assert!(app.status.contains("2048 bytes"));
        assert_eq!(
            app.draft_config.instructions.len(),
            2050,
            "keep the draft for correction"
        );
    }

    #[test]
    fn custom_instructions_editor_loads_and_reopening_discards_unsaved_edits() {
        let mut app = ready();
        let config = Config {
            instructions: "I write Rust.\nBe brief.".into(),
            onboarded: true,
            ..Config::default()
        };
        let _ = app.update(Message::Loaded(Ok(storage::Loaded {
            config,
            chats: Vec::new(),
            current: None,
        })));
        assert_eq!(app.instructions_editor.text(), "I write Rust.\nBe brief.");
        let _ = app.update(Message::Instructions(text_editor::Action::SelectAll));
        let _ = app.update(Message::Instructions(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        )));
        assert_eq!(app.draft_config.instructions, "x");
        app.app_window = None;
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.instructions_editor.text(), "I write Rust.\nBe brief.");
        assert_eq!(app.draft_config.instructions, app.config.instructions);
    }

    #[test]
    fn saving_a_new_profile_keeps_the_default_model() {
        let mut app = ready();
        let default_model = app.config.model.clone();
        let _ = app.update(Message::NewProfile);
        let _ = app.update(Message::ProfileName("Work".into()));
        let _ = app.update(Message::Model("other:1b".into()));
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.config.model, default_model);
        assert_eq!(app.config.profiles.len(), 1);
        assert_eq!(app.config.profiles[0].model, "other:1b");
        assert_eq!(
            app.profile_edit,
            ProfileEdit::Existing(app.config.profiles[0].id)
        );
        let _ = app.update(Message::EditProfile(0));
        assert_eq!(app.draft_config.model, default_model);
        let id = app.config.profiles[0].id;
        let _ = app.update(Message::EditProfile(id));
        let _ = app.update(Message::DeleteProfile(id));
        assert_eq!(app.config.profiles.len(), 1, "deleting waits for a yes");
        let _ = app.update(Message::DeleteProfileCancel);
        let _ = app.update(Message::DeleteProfileConfirm);
        assert_eq!(app.config.profiles.len(), 1, "Keep cancels it");
        let _ = app.update(Message::DeleteProfile(id));
        let _ = app.update(Message::DeleteProfileConfirm);
        assert!(app.config.profiles.is_empty());
        assert_eq!(app.profile_edit, ProfileEdit::Default);
    }

    #[test]
    fn the_starred_profile_starts_new_chats() {
        let mut app = ready();
        let cloud = with_cloud_profile(&mut app);
        app.draft_config.profiles = app.config.profiles.clone();
        let _ = app.update(Message::SetDefaultProfile(cloud));
        assert_eq!(app.config.default_profile, cloud);
        assert_eq!(app.draft_config.default_profile, cloud);
        exchange(&mut app, "hi", "ok");
        let _ = app.update(Message::NewChat);
        assert_eq!(app.chat.profile, cloud);

        // Saving other Model page edits keeps the star where it is.
        let _ = app.update(Message::EditProfile(0));
        let _ = app.update(Message::Model("other:1b".into()));
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.config.default_profile, cloud);
        assert_eq!(app.config.model, "other:1b");

        // Deleting the default hands the star back to the main profile.
        let _ = app.update(Message::DeleteProfile(cloud));
        let _ = app.update(Message::DeleteProfileConfirm);
        assert_eq!(app.config.default_profile, 0);
        assert!(app.config.validate().is_ok());
        let _ = app.update(Message::SetDefaultProfile(cloud));
        assert_eq!(
            app.config.default_profile, 0,
            "a missing profile can't be starred"
        );
    }

    #[test]
    fn deleting_another_profile_keeps_the_one_being_edited() {
        let mut app = ready();
        let cloud = with_cloud_profile(&mut app);
        let other = app.config.next_profile_id();
        app.config.profiles.push(bloop::config::Profile {
            id: other,
            name: "Other".into(),
            provider: Provider::Codex,
            endpoint: String::new(),
            model: String::new(),
        });
        let _ = app.update(Message::EditProfile(cloud));
        let _ = app.update(Message::DeleteProfile(other));
        let _ = app.update(Message::DeleteProfileConfirm);
        assert_eq!(app.profile_edit, ProfileEdit::Existing(cloud));
        assert_eq!(app.draft_config.model, "big");
    }

    fn first_launch() -> Island {
        let mut app = ready();
        // A real first launch has no notch yet; setup runs in the app window.
        app.window = None;
        let _ = app.update(loaded(Config::default()));
        app
    }

    #[test]
    fn first_launch_shows_onboarding_in_the_window() {
        let app = first_launch();
        assert!(app.onboarding.is_some());
        assert!(app.app_window.is_some());
        assert!(app.window.is_none());
    }

    #[test]
    fn finishing_opens_notch_and_home() {
        let mut app = first_launch();
        let _ = app.update(Message::ShowPage(AppPage::About));
        let _ = app.update(Message::OnboardSkip);
        assert!(app.config.onboarded);
        assert!(app.window.is_some());
        assert!(app.app_window.is_some());
        assert_eq!(app.page, AppPage::Home);
        assert_eq!(app.presence.message(), Some("I'm up here!"));
    }

    #[test]
    fn closing_mid_setup_opens_notch_and_keeps_setup_pending() {
        let mut app = first_launch();
        let id = app.app_window.unwrap();
        let _ = app.update(Message::CloseRequested(id));
        assert!(!app.config.onboarded);
        assert!(app.onboarding.is_none());
        assert!(app.window.is_some());
        assert!(app.app_window.is_none());
    }

    #[test]
    fn open_request_during_setup_keeps_the_step() {
        let mut app = first_launch();
        let _ = app.update(Message::OnboardNext);
        let step = app.onboarding.as_ref().unwrap().step();
        let window = app.app_window;
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.onboarding.as_ref().unwrap().step(), step);
        assert_eq!(app.app_window, window);
    }

    #[test]
    fn shortcut_during_setup_does_not_open_a_phantom_notch() {
        let mut app = first_launch();
        let id = app.app_window.unwrap();
        let _ = app.update(Message::Toggle);
        assert!(!app.is_open());
        let _ = app.update(Message::Platform(platform::Event::Toggle));
        assert!(!app.is_open());
        assert_eq!(app.app_window, Some(id));
        let _ = app.update(Message::Toggle);
        let _ = app.update(Message::Toggle);
        let _ = app.update(Message::CloseRequested(id));
        assert!(!app.is_open());
        assert!(app.window.is_some());
    }

    #[test]
    fn window_closed_clears_only_the_matching_app_window() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let id = app.app_window.unwrap();
        let _ = app.update(Message::WindowClosed(window::Id::unique()));
        assert_eq!(app.app_window, Some(id));
        let _ = app.update(Message::WindowClosed(id));
        assert!(app.app_window.is_none());
    }

    #[test]
    fn background_first_launch_still_onboards() {
        let mut app = Island::new(Launch::Background).0;
        let _ = app.update(loaded(Config::default()));
        assert!(app.onboarding.is_some());
        assert!(app.app_window.is_some());
    }

    #[test]
    fn onboarded_config_skips_setup() {
        let mut app = ready();
        let config = Config {
            onboarded: true,
            ..Config::default()
        };
        let _ = app.update(loaded(config));
        assert!(app.onboarding.is_none());
    }

    #[test]
    fn finishing_sets_onboarded_and_peeks() {
        let mut app = first_launch();
        for _ in 0..4 {
            let _ = app.update(Message::OnboardNext);
        }
        assert!(app.onboarding.as_ref().unwrap().is_last());
        let _ = app.update(Message::OnboardNext);
        assert!(app.onboarding.is_none());
        assert!(app.config.onboarded);
        assert_eq!(app.presence.state(), State::Peek);
        assert_eq!(app.presence.message(), Some("I'm up here!"));
    }

    #[test]
    fn skip_finishes_with_current_settings() {
        let mut app = first_launch();
        let _ = app.update(Message::OnboardSkip);
        assert!(app.onboarding.is_none());
        assert!(app.config.onboarded);
        assert_eq!(app.config.model, "qwen3:4b");
        assert_eq!(app.presence.state(), State::Peek);
    }

    #[test]
    fn invalid_draft_keeps_previous_config_on_finish() {
        let mut app = first_launch();
        let _ = app.update(Message::Endpoint("http://example.com".into()));
        let _ = app.update(Message::OnboardSkip);
        assert_eq!(app.config.endpoint, "http://127.0.0.1:11434");
        assert!(app.config.onboarded);
        assert!(app.onboarding.is_none());
    }

    fn found() -> Vec<Choice> {
        vec![
            Choice::ollama("http://127.0.0.1:11434", "llama3.2"),
            Choice::cli(Provider::ClaudeCode),
            Choice::cli(Provider::Codex),
        ]
    }

    #[test]
    fn detection_lists_every_found_provider() {
        let mut app = first_launch();
        let _ = app.update(Message::Detected(found()));
        assert_eq!(app.detection, Detection::Found(found()));
        // Current provider (Ollama) was found, so its first model is preselected.
        assert_eq!(app.draft_config.provider, Provider::Ollama);
        assert_eq!(app.draft_config.model, "llama3.2");
        let _ = app.update(Message::PickChoice(found()[2].clone()));
        assert_eq!(app.draft_config.provider, Provider::Codex);
        assert_eq!(app.draft_config.endpoint, "");
        assert_eq!(app.draft_config.model, "");
        let _ = app.update(Message::Detected(Vec::new()));
        assert_eq!(app.detection, Detection::NotFound);
    }

    #[test]
    fn late_detection_after_finish_is_ignored() {
        let mut app = first_launch();
        let _ = app.update(Message::OnboardSkip);
        let _ = app.update(Message::Detected(found()));
        assert_eq!(app.config.model, "qwen3:4b");
        assert_eq!(app.draft_config.model, "qwen3:4b");
        assert!(app.onboarding.is_none());
    }

    #[test]
    fn cli_provider_clears_endpoint_and_allows_blank_model() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let _ = app.update(Message::Provider(Provider::ClaudeCode));
        assert_eq!(app.draft_config.endpoint, "");
        let _ = app.update(Message::Model(String::new()));
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.status, "");
        assert_eq!(app.config.provider, Provider::ClaudeCode);
        assert!(app.config.model.is_empty());
    }

    #[test]
    fn icon_font_is_requested_once_the_first_window_opens() {
        let (mut app, _) = Island::new(Launch::Background);
        assert!(
            !app.icon_font_requested,
            "no window yet, so no renderer to load into"
        );
        let _ = app.update(loaded(Config {
            onboarded: true,
            ..Config::default()
        }));
        assert!(app.icon_font_requested);
        let _ = app.update(Message::OpenWindow);
        assert!(app.icon_font_requested);
    }

    #[test]
    fn header_uses_icons_once_the_font_loads() {
        let mut app = ready();
        assert!(!app.icons_ready);
        let _ = app.update(Message::FontLoaded(false));
        assert!(!app.icons_ready, "a failed font keeps the word labels");
        let _ = app.update(Message::FontLoaded(true));
        assert!(app.icons_ready);
    }

    #[test]
    fn dashboard_navigation() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.page, AppPage::Home);
        let _ = app.update(Message::ShowPage(AppPage::Appearance));
        assert_eq!(app.page, AppPage::Appearance);
        // Escape in the notch no longer walks back through settings pages.
        let _ = app.update(Message::Escape(app.window.unwrap()));
        assert_eq!(app.page, AppPage::Appearance);
    }

    #[test]
    fn notch_settings_icon_opens_dashboard_home() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        assert!(app.app_window.is_some());
        assert_eq!(app.page, AppPage::Home);
    }

    #[test]
    fn reopening_keeps_unsaved_model_draft() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let _ = app.update(Message::ShowPage(AppPage::Model));
        let _ = app.update(Message::Model("draft-model".into()));
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.draft_config.model, "draft-model");
        assert_eq!(app.page, AppPage::Model);
    }

    #[test]
    fn opening_a_closed_window_resets_the_draft_and_page() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let _ = app.update(Message::ShowPage(AppPage::Model));
        let _ = app.update(Message::Model("draft-model".into()));
        let id = app.app_window.unwrap();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.draft_config.model, app.config.model);
        assert_eq!(app.page, AppPage::Home);
    }

    #[test]
    fn appearance_changes_apply_live() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let _ = app.update(Message::SetNotchWidth(NotchWidth::L));
        assert_eq!(app.config.notch_width, NotchWidth::L);
    }

    #[test]
    fn dashboard_pages_have_titles_and_subset_icons() {
        assert_eq!(AppPage::Shortcut.title(), "Shortcut & startup");
        assert_eq!(AppPage::Home.icon(), Icon::Sparkles);
        assert_eq!(AppPage::About.icon(), Icon::Info);
    }

    #[test]
    fn look_changes_apply_and_persist_live() {
        let mut app = ready();
        let _ = app.update(Message::SetAccent(Accent::Teal));
        assert_eq!(app.config.accent, Accent::Teal);
        assert_eq!(app.draft_config.accent, Accent::Teal);
        assert_eq!(
            app.tokens().primary,
            iced::Color::from_rgb8(0x0d, 0x94, 0x88)
        );
        let _ = app.update(Message::SetTheme(ThemeChoice::System));
        assert_eq!(app.config.theme, ThemeChoice::System);
        let _ = app.update(Message::SetFeel(Feel::Calm));
        assert_eq!(app.config.motion, Feel::Calm);
        let _ = app.update(Message::SetReduceMotion(ReduceMotion::On));
        assert_eq!(app.config.reduce_motion, ReduceMotion::On);
        let _ = app.update(Message::SetMascot(false));
        assert!(!app.config.mascot);
    }

    #[test]
    fn notch_width_change_resizes_the_notch() {
        let mut app = ready();
        app.config.reduce_motion = ReduceMotion::On;
        let _ = app.present(Event::Shortcut);
        assert_eq!(app.size().width, 540.0);
        let _ = app.update(Message::SetNotchWidth(NotchWidth::L));
        assert_eq!(app.canvas_size().width, 600.0);
        assert_eq!(app.size().width, 600.0);
        let _ = app.update(Message::SetNotchPosition(NotchPosition::Right));
        assert_eq!(
            app.notch_rect().x,
            0.0,
            "panel fills the canvas, so x is 0 at any position"
        );
    }

    #[test]
    fn hover_delay_applies_to_presence() {
        let mut app = ready();
        let _ = app.update(Message::SetHoverDelay(300));
        assert_eq!(app.config.hover_delay_ms, 300);
        let start = Instant::now();
        let _ = app.present_at(Event::CursorEntered, start);
        let _ = app.present_at(Event::Tick, start + Duration::from_millis(200));
        assert_ne!(
            app.presence.state(),
            State::Hover,
            "still waiting at 200 ms"
        );
        let _ = app.present_at(Event::Tick, start + Duration::from_millis(310));
        assert_eq!(app.presence.state(), State::Hover);
    }

    fn sample_release() -> Release {
        Release {
            version: Version::parse("0.9.0").unwrap(),
            installer_url: "https://github.com/LaurentMaxhuni/bloop/releases/download/v0.9.0/bloop-setup-0.9.0.exe".into(),
            checksum_url: "https://github.com/LaurentMaxhuni/bloop/releases/download/v0.9.0/bloop-setup-0.9.0.exe.sha256".into(),
            size: 1000,
        }
    }

    fn ready_update() -> Message {
        Message::UpdateReady(Ok((
            Version::parse("0.9.0").unwrap(),
            PathBuf::from("C:/x/bloop-setup-0.9.0.exe"),
        )))
    }

    #[test]
    fn scheduled_check_failure_is_silent() {
        let mut app = ready();
        let _ = app.update(Message::UpdateChecked(false, Err("offline".into())));
        assert_eq!(app.update_state, UpdateState::Idle);
        assert!(app.update_status().contains("Up to date") || app.update_status().is_empty());
    }

    #[test]
    fn failed_scheduled_check_retries_soon_then_backs_off() {
        let mut app = ready();
        let start = Instant::now() + Duration::from_secs(60);
        let check_at = |app: &mut Island, at: Instant| {
            app.update_state = UpdateState::Idle;
            let _ = app.update(Message::UpdateTick(at));
            app.update_state == UpdateState::Checking
        };
        assert!(check_at(&mut app, start));
        // Offline at sign-in: try again in 5 minutes, not 6 hours.
        let _ = app.update(Message::UpdateChecked(false, Err("offline".into())));
        let first = Instant::now();
        assert!(!check_at(&mut app, first + Duration::from_secs(4 * 60)));
        assert!(check_at(&mut app, first + Duration::from_secs(5 * 60 + 1)));
        // Again offline: 10 minutes.
        let _ = app.update(Message::UpdateChecked(false, Err("offline".into())));
        let second = Instant::now();
        assert!(!check_at(&mut app, second + Duration::from_secs(9 * 60)));
        assert!(check_at(
            &mut app,
            second + Duration::from_secs(10 * 60 + 1)
        ));
        // A failed download is retried the same way, and success resets the delay.
        let _ = app.update(Message::UpdateReady(Err("connection reset".into())));
        let third = Instant::now();
        assert!(check_at(&mut app, third + Duration::from_secs(20 * 60 + 1)));
        let _ = app.update(Message::UpdateChecked(false, Ok(None)));
        assert_eq!(app.update_failures, 0);
        assert!(!check_at(
            &mut app,
            third + Duration::from_secs(5 * 60 * 60)
        ));
    }

    #[test]
    fn many_failures_never_wait_longer_than_the_schedule() {
        let mut app = ready();
        for _ in 0..40 {
            app.next_check = Instant::now() + UPDATE_EVERY * 2;
            let _ = app.update(Message::UpdateChecked(false, Err("offline".into())));
        }
        assert!(app.next_check <= Instant::now() + UPDATE_EVERY);
    }

    #[test]
    fn manual_check_failure_shows_error() {
        let mut app = ready();
        let _ = app.update(Message::UpdateChecked(true, Err("offline".into())));
        assert!(matches!(app.update_state, UpdateState::Failed(_)));
        assert_eq!(app.update_status(), "Couldn't check for updates: offline");
    }

    #[test]
    fn newer_release_starts_download() {
        let mut app = ready();
        let _ = app.update(Message::UpdateChecked(false, Ok(Some(sample_release()))));
        assert_eq!(
            app.update_state,
            UpdateState::Downloading(Version::parse("0.9.0").unwrap(), 0)
        );
        let _ = app.update(Message::UpdateProgress(
            Version::parse("0.9.0").unwrap(),
            42,
        ));
        assert_eq!(app.update_status(), "Downloading 0.9.0 (42 %)");
    }

    #[test]
    fn ready_peeks_unless_replying() {
        let mut app = ready();
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        assert!(app.request.is_some());
        let _ = app.update(ready_update());
        assert_ne!(
            app.presence.message(),
            Some("Update 0.9.0 ready — click to restart")
        );
        let id = app.request_id;
        let _ = app.update(Message::Finished(id, Ok(())));
        assert_eq!(
            app.presence.message(),
            Some("Update 0.9.0 ready — click to restart")
        );
        assert_eq!(app.update_status(), "0.9.0 ready");
    }

    #[test]
    fn pill_click_while_busy_reopens_panel() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        app.input = "hello".into();
        let _task = app.update(Message::Send);
        let _ = app.update(Message::Toggle);
        assert_eq!(
            app.presence.state(),
            State::Peek,
            "pill stays out while replying"
        );
        let _ = app.update(Message::OpenClick);
        assert_eq!(app.presence.state(), State::Open);
    }

    #[test]
    fn ready_peek_waits_for_the_panel_to_close() {
        let mut app = ready();
        let _ = app.update(Message::Toggle);
        assert!(app.is_open());
        let _ = app.update(ready_update());
        let _ = app.update(Message::Toggle);
        assert_eq!(app.presence.state(), State::Peek);
        assert_eq!(
            app.presence.message(),
            Some("Update 0.9.0 ready — click to restart")
        );
    }

    #[test]
    fn hover_pill_does_not_promise_a_restart() {
        let mut app = ready();
        let _ = app.update(ready_update());
        // The peek ends; afterwards hovering shows the pill without "click to restart".
        let _ = app.present_at(Event::Tick, Instant::now() + Duration::from_secs(10));
        assert_eq!(app.pill_status(), "Update 0.9.0 ready");
        app.status = "Could not save: disk full".into();
        assert_ne!(
            app.pill_status(),
            "Update 0.9.0 ready",
            "warnings still win"
        );
    }

    #[test]
    fn install_failure_keeps_bloop_open() {
        let mut app = ready();
        app.installer_launcher = |_| Err("blocked by antivirus".into());
        let _ = app.update(ready_update());
        let _ = app.update(Message::InstallUpdate);
        assert!(!app.quitting);
        assert!(matches!(app.update_state, UpdateState::Failed(_)));
        assert!(app.update_status().contains("blocked by antivirus"));
    }

    #[test]
    fn auto_update_off_skips_scheduled_checks() {
        let mut app = ready();
        let later = Instant::now() + Duration::from_secs(60);
        app.config.auto_update = false;
        let _ = app.update(Message::UpdateTick(later));
        assert_eq!(app.update_state, UpdateState::Idle);
        app.config.auto_update = true;
        let _ = app.update(Message::UpdateTick(later));
        assert_eq!(app.update_state, UpdateState::Checking);
        // Not again until six hours later.
        app.update_state = UpdateState::Idle;
        let _ = app.update(Message::UpdateTick(later + Duration::from_secs(60)));
        assert_eq!(app.update_state, UpdateState::Idle);
    }

    #[test]
    fn badge_names_the_cli() {
        let mut app = ready();
        app.config.provider = Provider::ClaudeCode;
        app.config.model = String::new();
        assert_eq!(app.badge_text(), "Cloud · Claude Code");
        app.config.model = "sonnet".into();
        assert_eq!(app.badge_text(), "Cloud · Claude Code · sonnet");
        app.config = Config::default();
        assert_eq!(app.badge_text(), "Local · qwen3:4b");
    }

    #[test]
    fn rerunning_setup_keeps_a_custom_endpoint() {
        let mut app = ready();
        app.config = Config {
            provider: Provider::OpenAi,
            endpoint: "https://api.example.com/v1".into(),
            model: "my-model".into(),
            onboarded: true,
            ..Config::default()
        };
        let _ = app.update(Message::ShowSetup);
        let _ = app.update(Message::Detected(found()));
        let _ = app.update(Message::OnboardSkip);
        assert_eq!(app.config.provider, Provider::OpenAi);
        assert_eq!(app.config.endpoint, "https://api.example.com/v1");
        assert_eq!(app.config.model, "my-model");
    }

    #[test]
    fn rerunning_setup_keeps_the_cli_model() {
        let mut app = ready();
        app.config = Config {
            provider: Provider::ClaudeCode,
            endpoint: String::new(),
            model: "sonnet".into(),
            onboarded: true,
            ..Config::default()
        };
        let _ = app.update(Message::ShowSetup);
        let _ = app.update(Message::Detected(found()));
        let _ = app.update(Message::OnboardSkip);
        assert_eq!(app.config.provider, Provider::ClaudeCode);
        assert_eq!(app.config.model, "sonnet");
    }

    #[test]
    fn finishing_setup_with_a_cli_keeps_it() {
        let mut app = first_launch();
        let _ = app.update(Message::Detected(found()));
        let _ = app.update(Message::PickChoice(found()[1].clone()));
        let _ = app.update(Message::OnboardSkip);
        assert_eq!(app.config.provider, Provider::ClaudeCode);
        assert!(app.config.onboarded);
    }

    #[test]
    fn show_setup_again_reopens_step_one() {
        let mut app = first_launch();
        let _ = app.update(Message::OnboardSkip);
        let _ = app.update(Message::ShowSetup);
        assert_eq!(app.onboarding.as_ref().unwrap().step(), Step::Welcome);
        assert!(!app.config.onboarded);
        assert!(app.app_window.is_some());
        assert!(app.window.is_some());
    }

    fn loaded_onboarded() -> Message {
        Message::Loaded(Ok(storage::Loaded {
            config: Config {
                onboarded: true,
                ..Config::default()
            },
            ..Default::default()
        }))
    }

    #[test]
    fn background_start_opens_only_the_notch() {
        let mut app = Island::new(Launch::Background).0;
        let _ = app.update(loaded_onboarded());
        assert!(app.window.is_some());
        assert!(app.app_window.is_none());
    }

    #[test]
    fn manual_start_opens_notch_and_window() {
        let mut app = Island::new(Launch::Manual).0;
        let _ = app.update(loaded_onboarded());
        assert!(app.window.is_some());
        assert!(app.app_window.is_some());
    }

    #[test]
    fn open_window_twice_keeps_one_window() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let first = app.app_window;
        let _ = app.update(Message::OpenWindow);
        assert_eq!(app.app_window, first);
    }

    #[test]
    fn closing_app_window_keeps_running() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let id = app.app_window.unwrap();
        let _ = app.update(Message::CloseRequested(id));
        assert!(app.app_window.is_none());
        assert!(!app.quitting);
    }

    /// Cursor enters window `id`, then the hover delay passes.
    fn hover_in(app: &mut Island, id: window::Id) {
        let _ = app.update(Message::Cursor(id, true));
        let _ = app.update(Message::Deadline(
            Instant::now() + Duration::from_millis(200),
        ));
    }

    #[test]
    fn app_window_cursor_does_not_touch_the_notch() {
        let mut app = ready();
        let _ = app.update(Message::OpenWindow);
        let id = app.app_window.unwrap();
        hover_in(&mut app, id);
        assert_eq!(app.presence.state(), State::Sliver);
        // The same hover over the notch does pop the pill.
        let notch = app.window.unwrap();
        hover_in(&mut app, notch);
        assert_eq!(app.presence.state(), State::Hover);
    }

    #[test]
    fn app_window_focus_loss_leaves_the_notch_open() {
        let mut app = ready();
        let notch = app.window.unwrap();
        let _ = app.update(Message::Toggle);
        assert!(app.is_open());
        let _ = app.update(Message::OpenWindow);
        let id = app.app_window.unwrap();
        let _ = app.update(Message::Focus(id, false));
        assert!(app.is_open());
        // The notch losing focus still dismisses it.
        let _ = app.update(Message::Focus(notch, false));
        assert!(!app.is_open());
    }

    #[test]
    fn escape_in_app_window_leaves_the_notch_alone() {
        let mut app = ready();
        let notch = app.window.unwrap();
        let _ = app.update(Message::Cursor(notch, true));
        let _ = app.update(Message::Deadline(
            Instant::now() + Duration::from_millis(200),
        ));
        let _ = app.update(Message::OpenClick);
        assert!(app.is_open());
        let _ = app.update(Message::OpenWindow);
        let id = app.app_window.unwrap();
        let _ = app.update(Message::Escape(id));
        assert!(app.is_open());
    }

    #[test]
    fn closing_the_notch_still_quits() {
        let mut app = ready();
        let notch = app.window.unwrap();
        let _ = app.update(Message::CloseRequested(notch));
        assert!(app.quitting);
    }
}

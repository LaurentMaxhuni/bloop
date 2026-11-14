//! The app window's dashboard: a fixed sidebar of pages and a scrolling page area.
//! Appearance applies live; the Model and Shortcut & startup pages keep a Save button.

use super::{Island, Message, UpdateState, field};
use crate::mascot::Mood;
use crate::platform;
use crate::{
    icons::{self, Icon},
    style::{self, MEDIUM, Variant},
};
use bloop::{
    config::{
        Accent, Config, HOVER_DELAYS_MS, MAIN_PROFILE, MAX_INSTRUCTIONS, MAX_PROFILES,
        NotchPosition, NotchWidth, Profile, ReduceMotion, ThemeChoice,
    },
    config::{Coat, Eyewear, Hat, Look, Outfit},
    motion::Feel,
    update::Version,
};
use iced::{
    Alignment, Border, Element, Length, Task,
    keyboard::{Key, Modifiers, key},
    widget::{Space, button, column, container, row, scrollable, text, text_editor, toggler},
};

/// Sidebar width and the page content's maximum width, in logical pixels.
const SIDEBAR_WIDTH: f32 = 200.0;
const CONTENT_MAX_WIDTH: f32 = 640.0;

/// Which of the two global shortcuts a recording replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutSlot {
    /// Opens and closes the notch.
    Toggle,
    /// Ask about the clipboard.
    Ask,
}

/// A shortcut being recorded from key presses in the app window.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub slot: ShortcutSlot,
    /// The modifiers held right now, such as `Ctrl+Alt`.
    pub held: String,
    /// The last valid combination pressed; saved when recording stops.
    pub captured: Option<String>,
}

impl Recording {
    pub fn new(slot: ShortcutSlot) -> Self {
        Self {
            slot,
            held: String::new(),
            captured: None,
        }
    }

    /// What the recording box shows.
    pub fn label(&self) -> String {
        match &self.captured {
            Some(keys) => format!("{keys} · Enter to save"),
            None if !self.held.is_empty() => format!("{}+…", self.held),
            None => "Press keys…".into(),
        }
    }
}

/// A keyboard event the recording listens to.
#[derive(Debug, Clone)]
pub enum RecordInput {
    Press(Key, key::Physical, Modifiers),
    Modifiers(Modifiers),
}

/// What one key press means while recording.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedKey {
    /// Escape or Enter: stop recording.
    Stop,
    /// Only modifiers so far; the text shows the ones held.
    Held(String),
    /// A usable combination, in the form `platform::validate_shortcut` accepts.
    Combo(String),
    /// Not a usable shortcut; the text says why.
    Invalid(String),
}

/// `Ctrl+Alt+Shift+Super`, in that order, for the modifiers in `modifiers`.
pub fn modifier_names(modifiers: Modifiers) -> String {
    [
        (modifiers.control(), "Ctrl"),
        (modifiers.alt(), "Alt"),
        (modifiers.shift(), "Shift"),
        (modifiers.logo(), "Super"),
    ]
    .iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| *name)
    .collect::<Vec<_>>()
    .join("+")
}

/// Turns one key press into a shortcut. The physical key names it, so the shortcut means the
/// same key whatever the keyboard layout; `global_hotkey` registers keys the same way.
pub fn record_key(key: &Key, physical: key::Physical, mut modifiers: Modifiers) -> RecordedKey {
    use key::Named;
    let named = match key {
        Key::Named(named) => Some(*named),
        _ => None,
    };
    match named {
        Some(Named::Escape | Named::Enter) => return RecordedKey::Stop,
        // A modifier's own press may arrive before the modifier state includes it.
        Some(Named::Control) => modifiers |= Modifiers::CTRL,
        Some(Named::Alt | Named::AltGraph) => modifiers |= Modifiers::ALT,
        Some(Named::Shift) => modifiers |= Modifiers::SHIFT,
        Some(Named::Super | Named::Meta | Named::Hyper) => modifiers |= Modifiers::LOGO,
        _ => {}
    }
    if matches!(
        named,
        Some(
            Named::Control
                | Named::Alt
                | Named::AltGraph
                | Named::Shift
                | Named::Super
                | Named::Meta
                | Named::Hyper
        )
    ) {
        return RecordedKey::Held(modifier_names(modifiers));
    }
    let key::Physical::Code(code) = physical else {
        return RecordedKey::Invalid("bloop can't use that key in a shortcut.".into());
    };
    if !(modifiers.control() || modifiers.alt() || modifiers.logo()) {
        return RecordedKey::Invalid("Hold Ctrl, Alt or Super with the key.".into());
    }
    let name = format!("{code:?}");
    let name = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .filter(|rest| rest.len() == 1)
        .unwrap_or(&name);
    let shortcut = format!("{}+{name}", modifier_names(modifiers));
    match platform::validate_shortcut(&shortcut) {
        Ok(()) => RecordedKey::Combo(shortcut),
        Err(error) => RecordedKey::Invalid(error),
    }
}

/// Both shortcuts parse, and they differ. Save and recording check the same rules.
pub(super) fn validate_shortcuts(config: &Config) -> Result<(), String> {
    platform::validate_shortcut(&config.shortcut)?;
    let ask = config.ask_shortcut.trim();
    if !ask.is_empty() {
        platform::validate_shortcut(ask)?;
        if ask.eq_ignore_ascii_case(config.shortcut.trim()) {
            return Err("The two shortcuts need to be different.".into());
        }
    }
    Ok(())
}

/// Which dashboard page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Home,
    Chats,
    Appearance,
    Model,
    Shortcut,
    About,
}

impl AppPage {
    const ALL: [AppPage; 6] = [
        AppPage::Home,
        AppPage::Chats,
        AppPage::Appearance,
        AppPage::Model,
        AppPage::Shortcut,
        AppPage::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AppPage::Home => "Home",
            AppPage::Chats => "Chats",
            AppPage::Appearance => "Appearance",
            AppPage::Model => "Model",
            AppPage::Shortcut => "Shortcut & startup",
            AppPage::About => "About",
        }
    }

    /// Sidebar glyph from the icon subset.
    pub fn icon(self) -> Icon {
        match self {
            AppPage::Home => Icon::Sparkles,
            AppPage::Chats => Icon::MessagesSquare,
            AppPage::Appearance => Icon::Palette,
            AppPage::Model => Icon::Cpu,
            AppPage::Shortcut => Icon::SlidersHorizontal,
            AppPage::About => Icon::Info,
        }
    }
}

/// Which model profile the Model page is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileEdit {
    /// The default profile: the provider, endpoint and model new chats use.
    Default,
    Existing(u32),
    /// A profile that Save will add.
    New,
}

/// A profile in a pick list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOption {
    pub id: u32,
    pub label: String,
}

impl std::fmt::Display for ProfileOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn theme_label(theme: ThemeChoice) -> &'static str {
    match theme {
        ThemeChoice::Light => "Light",
        ThemeChoice::Dark => "Dark",
        ThemeChoice::System => "Follow Windows",
    }
}

impl Island {
    pub(super) fn dashboard_view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let body = match self.page {
            AppPage::Home => self.home_page(),
            AppPage::Chats => self.chats_page(),
            AppPage::Appearance => self.appearance_page(),
            AppPage::Model => self.model_page(),
            AppPage::Shortcut => self.shortcut_page(),
            AppPage::About => self.about_page(),
        };
        let page = column![text(self.page.title()).size(22).font(MEDIUM), body]
            .spacing(20)
            .max_width(CONTENT_MAX_WIDTH);
        let divider = container(Space::new())
            .width(1)
            .height(Length::Fill)
            .style(style::separator(t));
        row![
            self.sidebar(),
            divider,
            scrollable(container(page).padding(28).width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::scroll(t)),
        ]
        .height(Length::Fill)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let items = AppPage::ALL
            .iter()
            .fold(column![].spacing(4), |items, &page| {
                let glyph: Element<'_, Message> = if self.icons_ready {
                    text(page.icon().codepoint().to_string())
                        .font(icons::FONT)
                        .size(16)
                        .into()
                } else {
                    Space::new().width(16).into()
                };
                items.push(
                    button(
                        row![glyph, text(page.title()).size(14)]
                            .spacing(10)
                            .align_y(Alignment::Center),
                    )
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(style::btn(
                        t,
                        if page == self.page {
                            Variant::Secondary
                        } else {
                            Variant::Ghost
                        },
                    ))
                    .on_press(Message::ShowPage(page)),
                )
            });
        column![
            container(text("bloop").size(18).font(MEDIUM)).padding([4, 12]),
            items,
        ]
        .spacing(16)
        .padding(16)
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill)
        .into()
    }

    /// A Home row: label, current value and a button to the page that changes it.
    fn summary_row<'a>(
        &self,
        label: &'a str,
        value: String,
        extra: Option<Element<'a, Message>>,
        page: AppPage,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        let mut line = row![
            column![
                text(label).size(13).font(MEDIUM),
                text(value).size(13).color(t.muted_foreground),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(extra) = extra {
            line = line.push(extra);
        }
        line = line.push(self.icon_action(
            Icon::ChevronRight,
            page.title(),
            page.title(),
            Variant::Ghost,
            Some(Message::ShowPage(page)),
        ));
        container(line)
            .padding([12, 14])
            .width(Length::Fill)
            .style(style::card(t, t.background))
            .into()
    }

    fn home_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let install = matches!(self.update_state, UpdateState::Ready(..)).then(|| {
            self.icon_action(
                Icon::RefreshCw,
                "Restart to update",
                "Restart to update",
                Variant::Primary,
                self.request.is_none().then_some(Message::InstallUpdate),
            )
        });
        let update = self.update_status();
        let version = if update.is_empty() {
            format!("bloop {}", Version::current())
        } else {
            format!("bloop {} · {update}", Version::current())
        };
        let mut page = column![
            self.summary_row("Model", self.badge_text(), None, AppPage::Model),
            self.summary_row("Status", self.pill_status(), None, AppPage::Model),
            self.summary_row(
                "Chats",
                match self.chats.len() {
                    1 => "1 saved chat".into(),
                    n => format!("{n} saved chats"),
                },
                None,
                AppPage::Chats,
            ),
            self.summary_row(
                "Global shortcut",
                self.config.shortcut.clone(),
                None,
                AppPage::Shortcut,
            ),
            self.summary_row("Version", version, install, AppPage::About),
        ]
        .spacing(10);
        #[cfg(target_os = "linux")]
        if !self.config.tiling_rule_dismissed
            && let Some(kind) =
                crate::platform::linux::desktop::tiling(|name| std::env::var_os(name))
        {
            page = page.push(self.tiling_card(kind));
        }
        if !self.storage_warning.is_empty() {
            page = page
                .push(text(&self.storage_warning).size(13).color(t.destructive))
                .push(
                    self.button("Quit without saving", Variant::Destructive)
                        .on_press(Message::QuitWithoutSaving),
                );
        }
        page.into()
    }

    /// Linux tiling desktops: the rule that keeps the notch floating on top.
    #[cfg(target_os = "linux")]
    fn tiling_card(&self, kind: crate::platform::linux::desktop::Tiling) -> Element<'_, Message> {
        use crate::platform::linux::desktop::{config_file, rule};
        let t = self.tokens();
        let rule = rule(kind);
        column![
            text("Keep the notch on top").size(14),
            text(format!(
                "Your desktop tiles windows. Add this to {} so the notch floats above them:",
                config_file(kind)
            ))
            .size(12)
            .color(t.muted_foreground),
            text(rule).size(12).font(iced::Font::MONOSPACE),
            row![
                self.button("Copy", Variant::Primary)
                    .on_press(Message::CopyTilingRule(rule.into())),
                self.button("Dismiss", Variant::Outline)
                    .on_press(Message::DismissTilingRule),
            ]
            .spacing(8),
        ]
        .spacing(8)
        .into()
    }

    /// A row of buttons where the selected option is filled.
    fn choices<T: Copy + PartialEq + 'static>(
        &self,
        options: &[T],
        selected: T,
        label: impl Fn(T) -> String,
        on: impl Fn(T) -> Message,
    ) -> Element<'_, Message> {
        let t = self.tokens();
        options
            .iter()
            .fold(row![].spacing(6), |row, &option| {
                row.push(
                    button(text(label(option)).size(13).center().width(Length::Fill))
                        .padding([7, 10])
                        .width(Length::Fill)
                        .style(style::btn(
                            t,
                            if option == selected {
                                Variant::Secondary
                            } else {
                                Variant::Outline
                            },
                        ))
                        .on_press(on(option)),
                )
            })
            .into()
    }

    /// A titled group of controls on the Appearance page.
    fn section<'a>(
        &self,
        icon: Icon,
        title: &'a str,
        body: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        let mut heading = row![].spacing(8).align_y(Alignment::Center);
        if self.icons_ready {
            heading = heading.push(
                text(icon.codepoint().to_string())
                    .font(icons::FONT)
                    .size(16)
                    .color(t.primary),
            );
        }
        heading = heading.push(text(title).size(16).font(MEDIUM));
        column![heading, body].spacing(12).into()
    }

    fn appearance_page(&self) -> Element<'_, Message> {
        column![
            self.section(Icon::Palette, "Look", self.look_section()),
            self.section(Icon::Star, "Bloop", self.bloop_section()),
            self.section(Icon::Sparkles, "Motion", self.motion_section()),
            self.section(Icon::PanelTop, "Notch", self.notch_section()),
        ]
        .spacing(28)
        .into()
    }

    fn look_section(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let swatches =
            Accent::ALL.iter().fold(row![].spacing(8), |row, &accent| {
                let colour = style::Tokens::with_accent(self.light(), accent).primary;
                let selected = accent == self.config.accent;
                row.push(
                    button(
                        column![
                            // A ring with a gap around the chosen swatch, visible on any colour.
                            container(container(Space::new()).width(26).height(26).style(
                                move |_| container::Style {
                                    background: Some(colour.into()),
                                    border: iced::Border {
                                        radius: 13.0.into(),
                                        width: 1.0,
                                        color: t.border,
                                    },
                                    ..Default::default()
                                },
                            ))
                            .padding(3)
                            .style(move |_| container::Style {
                                border: iced::Border {
                                    radius: 17.0.into(),
                                    width: 2.0,
                                    color: if selected {
                                        t.foreground
                                    } else {
                                        iced::Color::TRANSPARENT
                                    },
                                },
                                ..Default::default()
                            }),
                            text(accent.to_string()).size(11),
                        ]
                        .spacing(4)
                        .align_x(Alignment::Center),
                    )
                    .padding(4)
                    .style(style::btn(t, Variant::Bare))
                    .on_press(Message::SetAccent(accent)),
                )
            });
        column![
            field(
                "Theme",
                self.choices(
                    &[ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System],
                    self.config.theme,
                    |theme| theme_label(theme).into(),
                    Message::SetTheme,
                ),
            ),
            field("Accent", swatches),
        ]
        .spacing(16)
        .into()
    }

    fn motion_section(&self) -> Element<'_, Message> {
        let t = self.tokens();
        column![
            field(
                "Feel",
                self.choices(
                    &Feel::ALL,
                    self.config.motion,
                    |feel| feel.to_string(),
                    Message::SetFeel,
                ),
            ),
            field(
                "Reduce motion",
                self.choices(
                    &[ReduceMotion::System, ReduceMotion::On, ReduceMotion::Off],
                    self.config.reduce_motion,
                    |choice| match choice {
                        ReduceMotion::System => "Follow Windows".into(),
                        ReduceMotion::On => "On".into(),
                        ReduceMotion::Off => "Off".into(),
                    },
                    Message::SetReduceMotion,
                ),
            ),
            text("Reduce motion makes the notch change size instantly.")
                .size(12)
                .color(t.muted_foreground),
        ]
        .spacing(16)
        .into()
    }

    fn notch_section(&self) -> Element<'_, Message> {
        column![
            field(
                "Width",
                self.choices(
                    &NotchWidth::ALL,
                    self.config.notch_width,
                    |width| format!("{width:?}"),
                    Message::SetNotchWidth,
                ),
            ),
            field(
                "Position",
                self.choices(
                    &NotchPosition::ALL,
                    self.config.notch_position,
                    |position| position.to_string(),
                    Message::SetNotchPosition,
                ),
            ),
            field(
                "Hover delay",
                self.choices(
                    &HOVER_DELAYS_MS,
                    self.config.hover_delay_ms,
                    |ms| format!("{:.2} s", f32::from(ms) / 1000.0),
                    Message::SetHoverDelay,
                ),
            ),
        ]
        .spacing(16)
        .into()
    }

    /// Dress Bloop: a live preview, then hat, glasses, outfit and colour.
    fn bloop_section(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let look = self.config.look;
        let mood = self.mood();
        let blink = self.blink_until.is_some_and(|until| self.now < until);
        let preview = button(self.draw_mascot(mood, self.reaction_age(mood), blink, 96.0))
            .padding(0)
            .style(style::btn(t, Variant::Bare))
            .on_press(Message::PokeMascot);
        let controls = column![
            field(
                "Hat",
                self.choices(
                    Hat::ALL,
                    look.hat,
                    |h| h.label().into(),
                    move |hat| { Message::SetLook(Look { hat, ..look }) }
                ),
            ),
            field(
                "Glasses",
                self.choices(
                    Eyewear::ALL,
                    look.eyewear,
                    |e| e.label().into(),
                    move |eyewear| Message::SetLook(Look { eyewear, ..look }),
                ),
            ),
            field(
                "Outfit",
                self.choices(
                    Outfit::ALL,
                    look.outfit,
                    |o| o.label().into(),
                    move |outfit| { Message::SetLook(Look { outfit, ..look }) }
                ),
            ),
            field(
                "Colour",
                self.choices(
                    Coat::ALL,
                    look.coat,
                    |c| c.label().into(),
                    move |coat| { Message::SetLook(Look { coat, ..look }) }
                ),
            ),
        ]
        .spacing(16)
        .width(Length::Fill);
        let side = column![
            container(preview)
                .padding(12)
                .style(style::card(t, t.background)),
            self.button("Surprise me", Variant::Outline)
                .on_press(Message::ShuffleLook),
        ]
        .spacing(10)
        .align_x(Alignment::Center);
        column![
            row![side, controls].spacing(24),
            toggler(self.config.mascot)
                .label("Show Bloop in the notch")
                .text_size(14)
                .on_toggle(Message::SetMascot),
            text("Click Bloop to make it giggle.")
                .size(12)
                .color(t.muted_foreground),
        ]
        .spacing(16)
        .into()
    }

    /// The validation error from the last Save, then the Save button.
    fn save_row(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut save = column![].spacing(10);
        if !self.status.is_empty() {
            save = save.push(text(&self.status).size(13).color(t.destructive));
        }
        save.push(
            self.button("Save changes", Variant::Primary)
                .on_press_maybe((!self.loading).then_some(Message::SaveSettings)),
        )
        .into()
    }

    fn model_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut page = column![
            text("Choose the model bloop talks to. New chats start on the starred profile; switch a chat to another one from the notch.")
                .size(13)
                .color(t.muted_foreground),
            self.profile_list(),
            text(match self.profile_edit {
                ProfileEdit::New => "New profile".to_owned(),
                _ => format!(
                    "Editing {}",
                    self.config.profile_name(self.editing_id()).unwrap_or_default()
                ),
            })
            .size(16)
            .font(MEDIUM),
        ]
        .spacing(16);
        if self.profile_edit != ProfileEdit::Default {
            page = page.push(field(
                "Profile name",
                self.input("Work, Offline, …", &self.profile_name, Message::ProfileName),
            ));
        }
        page.push(self.model_fields())
            .push(self.connection_check())
            .push(
                text(
                    "Keys are stored in Windows Credential Manager, never in bloop's files, and are only sent to the server they were saved for. Remote endpoints require HTTPS."
                )
                .size(12)
                .color(t.muted_foreground),
            )
            .push(field(
                "Custom instructions",
                column![
                    text_editor(&self.instructions_editor)
                        .on_action(Message::Instructions)
                        .height(128)
                        .padding(10)
                        .size(14)
                        .font(style::REGULAR)
                        .style(style::editor(t)),
                    row![
                        text("Tell Bloop about yourself or how to reply. Sent with every message.")
                            .size(12)
                            .color(t.muted_foreground)
                            .width(Length::Fill),
                        text(format!("{} / {MAX_INSTRUCTIONS}", self.draft_config.instructions.len()))
                            .size(12)
                            .color(if self.draft_config.instructions.len() > MAX_INSTRUCTIONS {
                                t.destructive
                            } else {
                                t.muted_foreground
                            }),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            ))
            .push(self.save_row())
            .into()
    }

    /// Test connection and what it found.
    fn connection_check(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut check = column![
            row![
                self.icon_action(
                    Icon::Plug,
                    "Test connection",
                    "Test connection",
                    Variant::Outline,
                    self.testing.is_none().then_some(Message::TestConnection),
                ),
                text(if self.testing.is_some() {
                    "Testing…"
                } else {
                    ""
                })
                .size(13)
                .color(t.muted_foreground),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        ]
        .spacing(6);
        if let Some(report) = &self.connection {
            check = check.push(text(&report.summary).size(13).color(if report.ok {
                t.foreground
            } else {
                t.destructive
            }));
            if let Some(fix) = &report.fix {
                check = check.push(text(fix).size(12).color(t.muted_foreground));
            }
        }
        if !self.key_draft.trim().is_empty() {
            check = check.push(
                text("Save first to test with the key you pasted.")
                    .size(12)
                    .color(t.muted_foreground),
            );
        }
        check.into()
    }

    /// Profile options for pick lists: the default first, then the saved ones.
    pub(super) fn profile_options(&self) -> Vec<ProfileOption> {
        std::iter::once(0)
            .chain(self.config.profiles.iter().map(|p| p.id))
            .filter_map(|id| {
                let config = self.config.with_profile(id)?;
                Some(ProfileOption {
                    id,
                    label: format!(
                        "{} — {}",
                        self.config.profile_name(id)?,
                        self.describe_model(&config, None)
                    ),
                })
            })
            .collect()
    }

    /// The id the Model page is editing; a new profile has none yet, so it reads as 0.
    fn editing_id(&self) -> u32 {
        match self.profile_edit {
            ProfileEdit::Existing(id) => id,
            _ => 0,
        }
    }

    /// Every profile as a row: a star that makes it the default, its name and model, and
    /// icons to edit and delete it. The main profile can't be deleted.
    fn profile_list(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let default = self.config.default_profile();
        let mut list = column![].spacing(6);
        for id in std::iter::once(0).chain(self.config.profiles.iter().map(|p| p.id)) {
            let (Some(name), Some(config)) =
                (self.config.profile_name(id), self.config.with_profile(id))
            else {
                continue;
            };
            let editing = match self.profile_edit {
                ProfileEdit::Default => id == 0,
                ProfileEdit::Existing(edit) => edit == id,
                ProfileEdit::New => false,
            };
            let star: Element<'_, Message> = if id == default {
                self.tip(
                    container(self.glyph(Icon::Star, "Default", t.primary)).padding([7, 9]),
                    "New chats start here",
                )
            } else {
                self.icon_action(
                    Icon::Star,
                    "Make default",
                    "Make default",
                    Variant::Ghost,
                    Some(Message::SetDefaultProfile(id)),
                )
            };
            let mut line = row![
                star,
                column![
                    text(name).size(14).font(MEDIUM),
                    text(self.describe_model(&config, None))
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(2)
                .width(Length::Fill),
                self.icon_action(
                    Icon::Pencil,
                    "Edit",
                    "Edit",
                    if editing {
                        Variant::Secondary
                    } else {
                        Variant::Ghost
                    },
                    (!editing).then_some(Message::EditProfile(id)),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if id != 0 {
                line = line.push(self.icon_action(
                    Icon::Trash,
                    "Delete",
                    "Delete",
                    Variant::Ghost,
                    Some(Message::DeleteProfile(id)),
                ));
            }
            let mut card = column![line].spacing(8);
            if self.confirm_profile_delete == Some(id) {
                card = card.push(
                    row![
                        text(format!("Delete {name}? Chats that use it can't continue."))
                            .size(13)
                            .width(Length::Fill),
                        self.button("Keep", Variant::Outline)
                            .on_press(Message::DeleteProfileCancel),
                        self.button("Delete", Variant::Destructive)
                            .on_press(Message::DeleteProfileConfirm),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            list = list.push(container(card).padding([6, 10]).width(Length::Fill).style(
                style::card(t, if editing { t.secondary } else { t.background }),
            ));
        }
        let full = self.config.profiles.len() >= MAX_PROFILES;
        let add = self.icon_action(
            Icon::Plus,
            "New profile",
            if full {
                "Profile limit reached"
            } else {
                "New profile from these settings"
            },
            Variant::Outline,
            (!full && self.profile_edit != ProfileEdit::New).then_some(Message::NewProfile),
        );
        field("Profiles", list.push(add))
    }

    /// An icon in the icon font, or `word` until it loads.
    fn glyph<'a>(&self, icon: Icon, word: &'a str, color: iced::Color) -> Element<'a, Message> {
        if self.icons_ready {
            text(icon.codepoint().to_string())
                .font(icons::FONT)
                .size(16)
                .color(color)
                .into()
        } else {
            text(word).size(13).font(MEDIUM).color(color).into()
        }
    }

    /// Loads profile `id` into the Model page's fields.
    pub(super) fn edit_profile(&mut self, id: u32) {
        let Some(config) = self.config.with_profile(id) else {
            return;
        };
        self.draft_config.provider = config.provider;
        self.draft_config.endpoint = config.endpoint;
        self.draft_config.model = config.model;
        self.profile_edit = if id == 0 {
            ProfileEdit::Default
        } else {
            ProfileEdit::Existing(id)
        };
        self.profile_name = self.config.profile_name(id).unwrap_or_default().into();
        self.key_draft.clear();
        self.status.clear();
        self.forget_connection_test();
        self.refresh_key_status();
    }

    /// Starts a new profile from the fields as they are.
    pub(super) fn new_profile(&mut self) {
        if self.config.profiles.len() >= MAX_PROFILES {
            self.status = format!("Keep at most {MAX_PROFILES} saved profiles.");
            return;
        }
        self.profile_edit = ProfileEdit::New;
        self.profile_name = "New profile".into();
        self.key_draft.clear();
        self.status.clear();
    }

    /// Deletes the profile waiting for confirmation. If it was the default, the main
    /// profile becomes the default again.
    pub(super) fn delete_profile(&mut self) -> Task<Message> {
        let Some(id) = self.confirm_profile_delete.take() else {
            return Task::none();
        };
        self.config.profiles.retain(|p| p.id != id);
        self.draft_config.profiles.retain(|p| p.id != id);
        // Its id stays used: chats that kept it must not reach a later profile.
        self.config.last_profile_id = self.config.last_profile_id.max(id);
        self.draft_config.last_profile_id = self.config.last_profile_id;
        if self.config.default_profile == id {
            self.config.default_profile = 0;
        }
        self.draft_config.default_profile = self.config.default_profile;
        if self.profile_edit == ProfileEdit::Existing(id) {
            self.edit_profile(0);
        }
        self.persist()
    }

    /// Stars profile `id` for new chats and saves at once, like the Appearance settings.
    /// Unsaved edits on the Model page stay unsaved.
    pub(super) fn set_default_profile(&mut self, id: u32) -> Task<Message> {
        if self.config.profile_name(id).is_none() {
            return Task::none();
        }
        self.config.default_profile = id;
        self.draft_config.default_profile = id;
        self.persist()
    }

    /// The config Save would store: the drafts, with the profile being edited written into
    /// its place and the default model left alone. Checks everything first.
    pub(super) fn settings_to_save(&self) -> Result<Config, String> {
        let mut config = self.draft_config.clone();
        config.instructions = config.instructions.trim().to_owned();
        config.profiles = self.config.profiles.clone();
        config.default_profile = self.config.default_profile;
        let id = match self.profile_edit {
            ProfileEdit::Default => None,
            ProfileEdit::Existing(id) => Some(id),
            ProfileEdit::New => Some(self.config.next_profile_id()),
        };
        config.last_profile_id = self.config.last_profile_id;
        if self.profile_edit == ProfileEdit::New
            && let Some(id) = id
        {
            config.last_profile_id = config.last_profile_id.max(id);
        }
        if let Some(id) = id {
            let profile = Profile {
                id,
                name: self.profile_name.trim().to_owned(),
                provider: self.draft_config.provider,
                endpoint: self.draft_config.endpoint.trim().to_owned(),
                model: self.draft_config.model.trim().to_owned(),
            };
            profile.validate()?;
            if config
                .profiles
                .iter()
                .any(|p| p.id != id && p.name.eq_ignore_ascii_case(&profile.name))
                || profile.name.eq_ignore_ascii_case(MAIN_PROFILE)
            {
                return Err("Another profile already has that name.".into());
            }
            config.provider = self.config.provider;
            config.endpoint = self.config.endpoint.clone();
            config.model = self.config.model.clone();
            match config.profiles.iter_mut().find(|p| p.id == id) {
                Some(existing) => *existing = profile,
                None => config.profiles.push(profile),
            }
        }
        config.validate()?;
        validate_shortcuts(&config)?;
        Ok(config)
    }

    /// After Save: a new profile becomes the one being edited, and the fields keep showing it.
    pub(super) fn saved_profile(&mut self) {
        if self.profile_edit == ProfileEdit::New
            && let Some(id) = self.config.profiles.iter().map(|p| p.id).max()
        {
            self.profile_edit = ProfileEdit::Existing(id);
        }
        let id = match self.profile_edit {
            ProfileEdit::Existing(id) => id,
            _ => 0,
        };
        self.draft_config = self
            .config
            .with_profile(id)
            .unwrap_or_else(|| self.config.clone());
        self.instructions_editor = text_editor::Content::with_text(&self.draft_config.instructions);
    }

    /// Starts recording `slot`, or stops and saves when it is already recording.
    pub(super) fn toggle_recording(&mut self, slot: ShortcutSlot) -> Task<Message> {
        if self.recording.as_ref().is_some_and(|r| r.slot == slot) {
            return self.finish_recording();
        }
        self.status.clear();
        self.recording = Some(Recording::new(slot));
        Task::none()
    }

    /// One key event from the app window while recording.
    pub(super) fn record_input(&mut self, input: RecordInput) -> Task<Message> {
        let Some(recording) = self.recording.as_mut() else {
            return Task::none();
        };
        match input {
            RecordInput::Modifiers(modifiers) => recording.held = modifier_names(modifiers),
            RecordInput::Press(key, physical, modifiers) => {
                match record_key(&key, physical, modifiers) {
                    RecordedKey::Stop => return self.finish_recording(),
                    RecordedKey::Held(held) => recording.held = held,
                    RecordedKey::Combo(shortcut) => {
                        recording.captured = Some(shortcut);
                        self.status.clear();
                    }
                    RecordedKey::Invalid(error) => self.status = error,
                }
            }
        }
        Task::none()
    }

    /// Stops recording. A combination that differs from the saved one is validated, then
    /// saved and registered at once; an invalid one shows why and changes nothing.
    pub(super) fn finish_recording(&mut self) -> Task<Message> {
        let Some(recording) = self.recording.take() else {
            return Task::none();
        };
        let Some(shortcut) = recording.captured else {
            return Task::none();
        };
        let mut config = self.config.clone();
        let (saved, draft) = match recording.slot {
            ShortcutSlot::Toggle => (&mut config.shortcut, &mut self.draft_config.shortcut),
            ShortcutSlot::Ask => (
                &mut config.ask_shortcut,
                &mut self.draft_config.ask_shortcut,
            ),
        };
        if *saved == shortcut {
            *draft = shortcut;
            return Task::none();
        }
        *saved = shortcut.clone();
        if let Err(error) = validate_shortcuts(&config) {
            self.status = error;
            self.react(Mood::Oops, 1.2);
            return Task::none();
        }
        *draft = shortcut;
        self.config = config;
        self.status.clear();
        self.react(Mood::Happy, 1.2);
        self.apply_shortcuts();
        self.persist()
    }

    /// The text field and its Record button, or the recording box while it listens.
    fn shortcut_control<'a>(
        &'a self,
        slot: ShortcutSlot,
        placeholder: &'a str,
        value: &'a str,
        on_input: fn(String) -> Message,
    ) -> Element<'a, Message> {
        let t = self.tokens();
        match &self.recording {
            Some(recording) if recording.slot == slot => row![
                container(text(recording.label()).size(14))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        border: Border {
                            color: t.ring,
                            width: 2.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }),
                self.button("Done", Variant::Primary)
                    .on_press(Message::RecordShortcut(slot)),
            ],
            _ => row![
                self.input(placeholder, value, on_input).width(Length::Fill),
                self.button("Record", Variant::Outline)
                    .on_press_maybe((!self.loading).then_some(Message::RecordShortcut(slot))),
            ],
        }
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    fn shortcut_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let hint = |slot| {
            if self.recording.as_ref().is_some_and(|r| r.slot == slot) {
                "Hold Ctrl, Alt or Super and press a key. Enter or Esc saves it."
            } else {
                ""
            }
        };
        let toggle_hint = match hint(ShortcutSlot::Toggle) {
            "" => "Opens and closes the notch from anywhere.",
            recording => recording,
        };
        let ask_hint = match hint(ShortcutSlot::Ask) {
            "" => {
                "Copy some text, press this, and ask about it: the notch opens with the text attached. Leave it empty to turn it off."
            }
            recording => recording,
        };
        let mut page = column![
            field(
                "Global shortcut",
                self.shortcut_control(
                    ShortcutSlot::Toggle,
                    "Ctrl+Alt+B",
                    &self.draft_config.shortcut,
                    Message::Shortcut,
                ),
            ),
            text(toggle_hint).size(12).color(t.muted_foreground),
            field(
                "Ask about clipboard",
                self.shortcut_control(
                    ShortcutSlot::Ask,
                    "Ctrl+Alt+Shift+A",
                    &self.draft_config.ask_shortcut,
                    Message::AskShortcut,
                ),
            ),
            text(ask_hint).size(12).color(t.muted_foreground),
        ]
        .spacing(16);
        if !self.system_shortcuts.is_empty() {
            let keys = self
                .system_shortcuts
                .iter()
                .map(|(id, keys)| {
                    let name = if id == "ask" {
                        "Ask about clipboard"
                    } else {
                        "Global shortcut"
                    };
                    format!("{name}: {keys}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            page = page.push(text(keys).size(13)).push(
                text("Your desktop manages this shortcut.")
                    .size(12)
                    .color(t.muted_foreground),
            );
        }
        if !self.desktop_warning.is_empty() {
            page = page.push(text(&self.desktop_warning).size(13).color(t.destructive));
        }
        for note in &self.desktop_notes {
            page = page.push(text(note).size(13).color(t.muted_foreground));
        }
        page.push(self.save_row())
            .push(self.autostart_toggle())
            .into()
    }

    fn about_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let ready = matches!(self.update_state, UpdateState::Ready(..));
        let busy = matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Downloading(..)
        );
        let mut actions = row![self.icon_action(
            Icon::RefreshCw,
            "Check now",
            "Check for updates now",
            Variant::Outline,
            (!busy && !ready).then_some(Message::CheckUpdates { manual: true }),
        )]
        .spacing(8);
        if self.release_page.is_some() {
            actions = actions.push(self.icon_action(
                Icon::RefreshCw,
                "Open release page",
                "Open the release page to download the update",
                Variant::Primary,
                Some(Message::OpenReleasePage),
            ));
        }
        if ready {
            actions = actions.push(self.icon_action(
                Icon::RefreshCw,
                "Restart to update",
                "Restart to update",
                Variant::Primary,
                self.request.is_none().then_some(Message::InstallUpdate),
            ));
        }
        let folder = self
            .db_path
            .as_ref()
            .and_then(|path| path.parent())
            .map_or_else(|| "Unavailable".to_owned(), |dir| dir.display().to_string());
        let page = column![
            field(
                "Updates",
                column![
                    text(format!("bloop {}", Version::current())).size(14),
                    text(self.update_status())
                        .size(12)
                        .color(t.muted_foreground),
                    actions,
                    toggler(self.config.auto_update)
                        .label("Check for updates automatically")
                        .text_size(14)
                        .on_toggle(Message::SetAutoUpdate),
                ]
                .spacing(8),
            ),
            field(
                "Data folder",
                text(folder).size(13).color(t.muted_foreground),
            ),
            field(
                "Licence",
                text("bloop is open source under the MIT License (see LICENSE).")
                    .size(13)
                    .color(t.muted_foreground),
            ),
            field(
                "Setup",
                self.icon_action(
                    Icon::RotateCcw,
                    "Run setup again",
                    "Run setup again",
                    Variant::Outline,
                    Some(Message::ShowSetup),
                ),
            ),
        ]
        .spacing(20);
        #[cfg(debug_assertions)]
        let page = page.push(
            self.button("Peek test", Variant::Ghost)
                .on_press(Message::PeekTest),
        );
        page.into()
    }
}

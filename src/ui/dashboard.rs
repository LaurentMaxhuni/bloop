//! The app window's dashboard: a fixed sidebar of pages and a scrolling page area.
//! Appearance applies live; the Model and Shortcut & startup pages keep a Save button.

use super::{Island, Message, UpdateState, field};
use crate::{
    icons::{self, Icon},
    style::{self, MEDIUM, Variant},
};
use bloop::{
    config::{Accent, HOVER_DELAYS_MS, NotchPosition, NotchWidth, ReduceMotion, ThemeChoice},
    motion::Feel,
    update::Version,
};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text, toggler},
};

/// Sidebar width and the page content's maximum width, in logical pixels.
const SIDEBAR_WIDTH: f32 = 200.0;
const CONTENT_MAX_WIDTH: f32 = 640.0;

/// Which dashboard page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Home,
    Appearance,
    Model,
    Shortcut,
    About,
}

impl AppPage {
    const ALL: [AppPage; 5] = [
        AppPage::Home,
        AppPage::Appearance,
        AppPage::Model,
        AppPage::Shortcut,
        AppPage::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AppPage::Home => "Home",
            AppPage::Appearance => "Appearance",
            AppPage::Model => "Model",
            AppPage::Shortcut => "Shortcut & startup",
            AppPage::About => "About",
        }
    }

    /// Sidebar glyph from the icon subset; About has a word label only.
    pub fn icon(self) -> Option<Icon> {
        match self {
            AppPage::Home => Some(Icon::Sparkles),
            AppPage::Appearance => Some(Icon::Palette),
            AppPage::Model => Some(Icon::Cpu),
            AppPage::Shortcut => Some(Icon::SlidersHorizontal),
            AppPage::About => None,
        }
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
                let glyph: Element<'_, Message> = match page.icon() {
                    Some(icon) if self.icons_ready => text(icon.codepoint().to_string())
                        .font(icons::FONT)
                        .size(16)
                        .into(),
                    _ => Space::new().width(16).into(),
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
        line = line.push(
            self.button(page.title(), Variant::Outline)
                .on_press(Message::ShowPage(page)),
        );
        container(line)
            .padding([12, 14])
            .width(Length::Fill)
            .style(style::card(t, t.background))
            .into()
    }

    fn home_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let install = matches!(self.update_state, UpdateState::Ready(..)).then(|| {
            self.button("Restart to update", Variant::Primary)
                .on_press_maybe(self.request.is_none().then_some(Message::InstallUpdate))
                .into()
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
                "Global shortcut",
                self.config.shortcut.clone(),
                None,
                AppPage::Shortcut,
            ),
            self.summary_row("Version", version, install, AppPage::About),
        ]
        .spacing(10);
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
            toggler(self.config.mascot)
                .label("Show the mascot")
                .text_size(14)
                .on_toggle(Message::SetMascot),
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
        column![
            text("Choose the model bloop talks to.")
                .size(13)
                .color(t.muted_foreground),
            self.model_fields(),
            text(
                "Keys are stored in Windows Credential Manager, never in bloop's files. Remote endpoints require HTTPS."
            )
            .size(12)
            .color(t.muted_foreground),
            self.save_row(),
        ]
        .spacing(16)
        .into()
    }

    fn shortcut_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut page = column![
            field(
                "Global shortcut",
                self.input("Ctrl+Alt+B", &self.draft_config.shortcut, Message::Shortcut),
            ),
            text("Opens and closes the notch from anywhere.")
                .size(12)
                .color(t.muted_foreground),
        ]
        .spacing(16);
        if !self.desktop_warning.is_empty() {
            page = page.push(text(&self.desktop_warning).size(13).color(t.destructive));
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
        let mut actions =
            row![self.button("Check now", Variant::Outline).on_press_maybe(
                (!busy && !ready).then_some(Message::CheckUpdates { manual: true })
            ),]
            .spacing(8);
        if ready {
            actions = actions.push(
                self.button("Restart to update", Variant::Primary)
                    .on_press_maybe(self.request.is_none().then_some(Message::InstallUpdate)),
            );
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
                self.button("Run setup again", Variant::Outline)
                    .on_press(Message::ShowSetup),
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

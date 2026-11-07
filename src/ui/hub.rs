//! The customization hub: four tiles (Look, Motion, Notch, Model & shortcut), each opening
//! a small page. Look, Motion and Notch apply live; the Model page keeps its Save button.

use super::{Island, Message, field};
use crate::{
    icons::{self, Icon},
    style::{self, MEDIUM, Variant},
};
use bloop::{
    config::{Accent, HOVER_DELAYS_MS, NotchPosition, NotchWidth, ReduceMotion, ThemeChoice},
    motion::Feel,
};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text, toggler},
};

/// Which part of the hub is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubPage {
    Grid,
    Look,
    Motion,
    Notch,
    Model,
}

impl HubPage {
    fn title(self) -> &'static str {
        match self {
            HubPage::Grid => "Customize",
            HubPage::Look => "Look",
            HubPage::Motion => "Motion",
            HubPage::Notch => "Notch",
            HubPage::Model => "Model & shortcut",
        }
    }

    fn icon(self) -> Icon {
        match self {
            HubPage::Grid => Icon::SlidersHorizontal,
            HubPage::Look => Icon::Palette,
            HubPage::Motion => Icon::Sparkles,
            HubPage::Notch => Icon::PanelTop,
            HubPage::Model => Icon::Cpu,
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
    /// One line under each tile's title describing its current values.
    pub(super) fn tile_summary(&self, page: HubPage) -> String {
        let c = &self.config;
        match page {
            HubPage::Grid => String::new(),
            HubPage::Look => {
                let theme = match c.theme {
                    ThemeChoice::System => "Windows",
                    other => theme_label(other),
                };
                format!("{theme} · {}", c.accent)
            }
            HubPage::Motion => match c.reduce_motion {
                ReduceMotion::On => format!("{} · reduced", c.motion),
                _ => c.motion.to_string(),
            },
            HubPage::Notch => format!("{:?} · {}", c.notch_width, c.notch_position),
            HubPage::Model => c.provider.to_string(),
        }
    }

    pub(super) fn hub_view(&self, page: HubPage) -> Element<'_, Message> {
        let body = match page {
            HubPage::Grid => return self.grid(),
            HubPage::Look => self.look_page(),
            HubPage::Motion => self.motion_page(),
            HubPage::Notch => self.notch_page(),
            HubPage::Model => self.model_page(),
        };
        let t = self.tokens();
        let back = self.icon_button(
            Icon::ArrowLeft,
            "Back",
            "Back",
            Message::OpenHub(HubPage::Grid),
        );
        scrollable(
            column![
                row![back, text(page.title()).size(20).font(MEDIUM)]
                    .spacing(8)
                    .align_y(Alignment::Center),
                body,
            ]
            .spacing(16),
        )
        .height(Length::Fill)
        .style(style::scroll(t))
        .into()
    }

    fn grid(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let tile = |page: HubPage| {
            let glyph: Element<'_, Message> = if self.icons_ready {
                text(page.icon().codepoint().to_string())
                    .font(icons::FONT)
                    .size(22)
                    .color(t.primary)
                    .into()
            } else {
                Space::new().height(22).into()
            };
            button(
                column![
                    glyph,
                    text(page.title()).size(15).font(MEDIUM),
                    text(self.tile_summary(page))
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(6),
            )
            .padding(16)
            .width(Length::FillPortion(1))
            .height(120)
            .style(style::btn(t, Variant::Outline))
            .on_press(Message::OpenHub(page))
        };
        column![
            column![
                text("Customize").size(20).font(MEDIUM),
                text("Make bloop look and move the way you like.")
                    .size(13)
                    .color(t.muted_foreground),
            ]
            .spacing(2),
            row![tile(HubPage::Look), tile(HubPage::Motion)].spacing(12),
            row![tile(HubPage::Notch), tile(HubPage::Model)].spacing(12),
        ]
        .spacing(16)
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

    fn look_page(&self) -> Element<'_, Message> {
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

    fn motion_page(&self) -> Element<'_, Message> {
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

    fn notch_page(&self) -> Element<'_, Message> {
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

    fn model_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut form = column![
            text("Choose the model bloop talks to.")
                .size(13)
                .color(t.muted_foreground),
            self.model_fields(),
            field(
                "Global shortcut",
                self.input("Ctrl+Alt+B", &self.draft_config.shortcut, Message::Shortcut),
            ),
            text(
                "Keys are stored in Windows Credential Manager, never in bloop's files. Remote endpoints require HTTPS."
            )
            .size(12)
            .color(t.muted_foreground),
            self.autostart_toggle(),
            self.button("Show setup again", Variant::Ghost)
                .on_press(Message::ShowSetup),
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
        form.into()
    }
}

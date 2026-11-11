//! The first-launch setup screens, drawn in the app window.

use super::{Island, Message, field};
use crate::{
    mascot::Notch,
    style::{self, MEDIUM, Tokens, Variant},
};
use bloop::{
    config::{Provider, ThemeChoice},
    onboarding::{Detection, Onboarding, Step},
};
use iced::{
    Alignment, Color, Element, Length,
    widget::{Space, button, canvas, column, container, pick_list, row, scrollable, text, toggler},
};

/// Widest the setup column grows in the window, in logical pixels.
const SETUP_WIDTH: f32 = 560.0;

impl Island {
    pub(super) fn onboarding_view(&self) -> Element<'_, Message> {
        let Some(onboarding) = &self.onboarding else {
            return Space::new().into();
        };
        let body = match onboarding.step() {
            Step::Welcome => self.welcome(),
            Step::Model => self.model_step(),
            Step::HowItWorks => self.how_it_works(),
            Step::MakeItYours => self.make_it_yours(),
            Step::AllSet => self.all_set(),
        };
        // The welcome screen carries its own large mascot.
        let mascot: Element<'_, Message> = if onboarding.is_first() {
            Space::new().into()
        } else {
            container(self.mascot(48.0)).center_x(Length::Fill).into()
        };
        let column = column![
            mascot,
            scrollable(body)
                .height(Length::Fill)
                .style(style::scroll(self.tokens())),
            self.step_bar(onboarding),
        ]
        .spacing(14)
        .max_width(SETUP_WIDTH);
        container(column)
            .center_x(Length::Fill)
            .padding(24)
            .height(Length::Fill)
            .into()
    }

    /// "Start with Windows" switch, shared with Settings.
    pub(super) fn autostart_toggle(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut control = column![
            toggler(self.autostart)
                .label("Start with Windows")
                .text_size(14)
                .on_toggle(Message::SetAutostart),
        ]
        .spacing(6);
        if !self.autostart_error.is_empty() {
            control = control.push(text(&self.autostart_error).size(12).color(t.destructive));
        }
        control.into()
    }

    fn step_bar(&self, onboarding: &Onboarding) -> Element<'_, Message> {
        let t = self.tokens();
        let dots = (0..Onboarding::COUNT).fold(row![].spacing(6), |dots, index| {
            let color = if index == onboarding.index() {
                t.foreground
            } else {
                t.border
            };
            dots.push(
                container(Space::new())
                    .width(if index == onboarding.index() { 18 } else { 6 })
                    .height(6)
                    .style(move |_| container::Style {
                        background: Some(color.into()),
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
        });
        let mut actions = row![].spacing(8).align_y(Alignment::Center);
        if !onboarding.is_last() {
            actions = actions.push(
                self.button("Skip setup", Variant::Ghost)
                    .on_press(Message::OnboardSkip),
            );
        }
        if !onboarding.is_first() {
            actions = actions.push(
                self.button("Back", Variant::Outline)
                    .on_press(Message::OnboardBack),
            );
        }
        actions = actions.push(
            self.button(
                if onboarding.is_last() {
                    "Finish"
                } else {
                    "Next"
                },
                Variant::Primary,
            )
            .on_press(Message::OnboardNext),
        );
        row![dots, Space::new().width(Length::Fill), actions]
            .align_y(Alignment::Center)
            .into()
    }

    fn heading<'a>(&self, title: &'a str, line: &'a str) -> Element<'a, Message> {
        column![
            text(title).size(22).font(MEDIUM),
            text(line).size(14).color(self.tokens().muted_foreground),
        ]
        .spacing(4)
        .into()
    }

    /// Heading for the mascot screens, centered under it.
    fn centered_heading<'a>(&self, title: &'a str, line: &'a str) -> Element<'a, Message> {
        column![
            text(title)
                .size(22)
                .font(MEDIUM)
                .center()
                .width(Length::Fill),
            text(line)
                .size(14)
                .color(self.tokens().muted_foreground)
                .center()
                .width(Length::Fill),
        ]
        .spacing(4)
        .into()
    }

    fn welcome(&self) -> Element<'_, Message> {
        column![
            Space::new().height(24),
            self.mascot(96.0),
            self.centered_heading(
                "Hi, I'm bloop.",
                "A tiny assistant that lives at the top of your screen and talks to a model on your own computer.",
            ),
        ]
        .spacing(18)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    fn model_step(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let mut content = column![self.heading(
            "Your model",
            "Choose a model for Bloop. You can change it later in Settings.",
        )]
        .spacing(16);
        content = match &self.detection {
            Detection::Checking => content.push(
                text("Looking for Ollama, Unsloth Studio, Claude Code and Codex…")
                    .size(14)
                    .color(t.muted_foreground),
            ),
            Detection::Found(choices) => {
                let selected = choices
                    .iter()
                    .find(|c| {
                        c.provider == self.draft_config.provider
                            && c.endpoint == self.draft_config.endpoint
                            && c.model == self.draft_config.model
                    })
                    .cloned();
                let mut found = column![
                    row![
                        text("Found on this computer:").size(14).width(Length::Fill),
                        self.button("Retry", Variant::Ghost)
                            .on_press(Message::Detect),
                    ]
                    .align_y(Alignment::Center),
                    pick_list(choices.as_slice(), selected, Message::PickChoice)
                        .placeholder("Pick one")
                        .padding([8, 12])
                        .text_size(14)
                        .width(Length::Fill)
                        .style(style::select(t))
                        .menu_style(style::menu(t)),
                ]
                .spacing(12);
                if self.draft_config.provider.is_cli() {
                    found = found.push(
                        text(format!(
                            "Uses your {} sign-in. It can only chat here: no files, commands or tools.",
                            self.draft_config.provider
                        ))
                        .size(13)
                        .color(t.muted_foreground),
                    );
                }
                if let Some(key) = self.key_field() {
                    found = found.push(key);
                }
                content.push(found)
            }
            Detection::NotFound => content
                .push(
                    row![
                        text("Start Ollama or Unsloth Studio, install Claude Code or Codex, or enter an endpoint")
                            .size(14)
                            .color(t.muted_foreground)
                            .width(Length::Fill),
                        self.button("Retry", Variant::Outline)
                            .on_press(Message::Detect),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .push(self.model_fields()),
        };
        if !matches!(self.detection, Detection::NotFound) {
            content = if self.draft_config.provider == Provider::ChatGpt {
                content.push(self.model_fields())
            } else {
                content.push(
                    self.button("Use ChatGPT", Variant::Outline)
                        .on_press(Message::Provider(Provider::ChatGpt)),
                )
            };
        }
        content.into()
    }

    fn how_it_works(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let shortcut = if self.draft_config.shortcut.trim().is_empty() {
            "Ctrl+Alt+B".to_owned()
        } else {
            self.draft_config.shortcut.trim().to_owned()
        };
        let stage = |notch: Element<'static, Message>, caption: String| {
            column![
                container(notch)
                    .width(Length::Fill)
                    .height(80)
                    .align_x(Alignment::Center)
                    .style(style::card(t, t.secondary)),
                text(caption).size(13).center().width(Length::Fill),
            ]
            .spacing(8)
            .width(Length::FillPortion(1))
        };
        column![
            self.heading(
                "How it works",
                "bloop stays out of the way until you need it."
            ),
            row![
                stage(
                    mini_notch(t.foreground, None, 48.0, 4.0),
                    "Rest your mouse on the sliver".into()
                ),
                stage(
                    mini_notch(t.foreground, None, 96.0, 16.0),
                    "Click to chat".into()
                ),
                stage(
                    mini_notch(t.foreground, None, 96.0, 60.0),
                    format!("{shortcut} from anywhere")
                ),
            ]
            .spacing(12),
            field(
                "Global shortcut",
                self.input("Ctrl+Alt+B", &self.draft_config.shortcut, Message::Shortcut),
            ),
        ]
        .spacing(18)
        .into()
    }

    fn make_it_yours(&self) -> Element<'_, Message> {
        let preview = |light: bool| {
            let p = Tokens::new(light);
            let selected = self.config.theme != ThemeChoice::System && self.light() == light;
            button(
                column![
                    container(mini_notch(
                        p.background,
                        light.then_some(p.border),
                        110.0,
                        20.0
                    ))
                    .width(Length::Fill)
                    .height(70)
                    .align_x(Alignment::Center)
                    .style(style::card(p, p.secondary)),
                    text(if light { "Light" } else { "Dark" })
                        .size(13)
                        .font(MEDIUM),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .padding(8)
            .width(Length::FillPortion(1))
            .style(style::btn(
                self.tokens(),
                if selected {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                },
            ))
            .on_press(Message::SetLight(light))
        };
        column![
            self.heading("Make it yours", "Pick a look. You can switch any time."),
            row![preview(false), preview(true)].spacing(12),
            self.button(
                if self.config.theme == ThemeChoice::System {
                    "Following Windows"
                } else {
                    "Follow Windows instead"
                },
                if self.config.theme == ThemeChoice::System {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                },
            )
            .on_press(Message::SetTheme(ThemeChoice::System)),
            self.autostart_toggle(),
        ]
        .spacing(18)
        .into()
    }

    fn all_set(&self) -> Element<'_, Message> {
        column![
            Space::new().height(24),
            self.mascot(96.0),
            self.centered_heading("All set", "Hover the top of your screen anytime."),
        ]
        .spacing(18)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

/// A small notch silhouette hanging from the top of its box, for the step illustrations.
fn mini_notch(
    fill: Color,
    outline: Option<Color>,
    width: f32,
    height: f32,
) -> Element<'static, Message> {
    canvas(Notch {
        fill,
        outline,
        shoulder: (height * 0.6).clamp(3.0, 6.0),
        radius: (height / 2.0).min(12.0),
    })
    .width(width)
    .height(height)
    .into()
}

//! The first-start wizard (see `app::onboarding`) and the helper-programs
//! dialog it shares a step with.

use super::{label, label_w};
use crate::app::advanced::AdvancedMsg;
use crate::app::helpers::{HELPERS, HelpersMsg};
use crate::app::onboarding::{AI_STUDIO_KEYS, OnboardMsg, Step};
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::overlay::ModalParts;
use iced::font::Weight;
use iced::widget::{
    Column, button, checkbox, column, container, pick_list, row, scrollable, space, text_input,
};
use iced::{Alignment, Element, Length, Padding};

fn para<'a>(state: &'a Listen, s: &'static str) -> Element<'a, Message> {
    crate::ui::t_text(s)
        .font(crate::appearance::current_font())
        .size(metrics::font_sm())
        .color(state.tokens.muted)
        .into()
}

fn btn<'a>(
    state: &'a Listen,
    text: &'static str,
    primary: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let b = button(label_w(
        text,
        metrics::font_md(),
        if primary { iced::Color::WHITE } else { t.foreground },
        Weight::Semibold,
    ))
    .padding(Padding::from([8.0, 18.0]))
    .on_press_maybe(msg);
    if primary { b.style(style::primary_button(t)).into() } else { b.style(style::default_button(t)).into() }
}

/// A feature checkbox with its explanation; `msg` = None leaves it disabled.
fn feature<'a>(
    state: &'a Listen,
    name: &'static str,
    hint: &'static str,
    on: bool,
    msg: Option<fn(bool) -> Message>,
) -> Element<'a, Message> {
    let mut b = checkbox(on)
        .label(crate::i18n::tr(name))
        .size(18)
        .text_size(metrics::font_md())
        .font(super::font(Weight::Medium));
    if let Some(f) = msg {
        b = b.on_toggle(f);
    }
    column![b, container(para(state, hint)).padding(Padding { left: 28.0, ..Padding::ZERO })]
        .spacing(2)
        .into()
}

#[derive(Debug, Clone, PartialEq)]
struct Lang(&'static str, String);

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn welcome(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let mut options = vec![Lang("", crate::i18n::tr("System default").to_string())];
    options.extend(crate::i18n::LANGUAGES.iter().map(|(c, n)| Lang(c, (*n).to_string())));
    let current = state.config.settings.appearance.language.as_str();
    let selected = options.iter().find(|o| o.0 == current).cloned();
    column![
        label_w("Welcome to Listen", 28.0, t.foreground, Weight::Bold),
        para(
            state,
            "A music player for your Navidrome or Jellyfin server, YouTube Music and local files. \
             These few steps set up the extras; you can skip any of them and change everything later in Settings."
        ),
        label_w("Language", metrics::font_md(), t.foreground, Weight::Semibold),
        pick_list(options, selected, |l| Message::Onboard(OnboardMsg::Language(l.0.to_string()))).width(260),
    ]
    .spacing(metrics::space_md())
}

fn features(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings;
    let ytm_on = s.analysis.youtube_music && !s.analysis.shared_dir.trim().is_empty();
    column![
        label_w("Choose your features", 24.0, t.foreground, Weight::Bold),
        feature(
            state,
            "Auto-DJ",
            "Keeps the music going with similar songs when the queue runs out.",
            s.auto_dj.enabled,
            Some(|v| Message::Setting(Setting::AutoDj(v)))
        ),
        feature(
            state,
            "DJ transitions",
            "Mixes the songs the DJs pick with beat-matched blends instead of a gap.",
            s.mixing.enabled,
            Some(|v| Message::Setting(Setting::Mixing(v)))
        ),
        feature(
            state,
            "Analyse songs",
            "Measures tempo, beats, key and energy of your server's songs for the DJ transitions. Runs in the background.",
            s.analysis.enabled,
            Some(|v| Message::Setting(Setting::Analysis(v)))
        ),
        feature(
            state,
            "Analyse YouTube Music songs",
            "Fetches a temporary copy of each YouTube Music song to measure it, so those songs can be mixed too. Needs an analysis folder (next step).",
            ytm_on,
            Some(|v| Message::Setting(Setting::AnalysisYtm(v)))
        ),
        feature(
            state,
            "AI DJ",
            "Picks sets of songs and talks between them like a radio host. Needs your own Gemini API key (see the AI DJ step).",
            s.ai_dj.enabled,
            state.dj.has_key().then_some(|v| Message::Setting(Setting::AiDj(v)))
        ),
        feature(
            state,
            "Synced lyrics",
            "Looks up lyrics online when your server has none.",
            s.lyrics.fetch,
            Some(|v| Message::Setting(Setting::LyricsFetch(v)))
        ),
        feature(
            state,
            "Discord status",
            "Shows what you're listening to in your Discord profile.",
            s.window.discord_rpc,
            Some(|v| Message::Setting(Setting::Discord(v)))
        ),
        feature(
            state,
            "Tray icon",
            "Keeps Listen in the system tray.",
            s.window.tray,
            Some(|v| Message::Setting(Setting::Tray(v)))
        ),
        feature(
            state,
            "Check for updates",
            "Looks for a new version on GitHub when Listen starts. Nothing is installed without asking.",
            s.advanced.update_check_on_start,
            Some(|v| Message::Advanced(AdvancedMsg::CheckOnStart(v)))
        ),
    ]
    .spacing(metrics::space_md())
}

fn analysis(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let a = &state.config.settings.analysis;
    let folder = if a.shared_dir.trim().is_empty() {
        crate::i18n::tr("No folder chosen yet (results stay in Listen's data folder; YouTube Music songs are not analysed).")
            .to_string()
    } else {
        a.shared_dir.clone()
    };
    column![
        label_w("Folder for analysis data", 24.0, t.foreground, Weight::Bold),
        para(
            state,
            "DJ transitions need to know each song's tempo, beats and key. Listen measures them once and keeps \
             the results in this folder. Pick a synced folder (Nextcloud, Syncthing, …) and your other computers \
             reuse the results."
        ),
        container(label(folder, metrics::font_md(), t.foreground))
            .padding(metrics::space_md())
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(t.surface.into()),
                border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
                ..Default::default()
            }),
        btn(state, "Choose folder…", true, Some(Message::Setting(Setting::AnalysisDirPick))),
        feature(
            state,
            "Analyse YouTube Music songs",
            "Off by default. Listen then fetches a temporary copy of each YouTube Music song it is about to mix \
             and keeps only the measurements.",
            a.youtube_music && !a.shared_dir.trim().is_empty(),
            (!a.shared_dir.trim().is_empty()).then_some(|v| Message::Setting(Setting::AnalysisYtm(v)))
        ),
    ]
    .spacing(metrics::space_md())
}

fn ai_dj(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let has_key = state.dj.has_key();
    let mut c = column![
        label_w("AI DJ: your Gemini API key", 24.0, t.foreground, Weight::Bold),
        para(
            state,
            "The AI DJ uses Google's Gemini with your own key. Listen ships no key and has no server: requests go \
             from this computer straight to Google, and the key stays in your system keyring."
        ),
        para(state, "1. Open Google AI Studio and sign in with a Google account."),
        para(state, "2. Click “Create API key” (pick a project or let AI Studio create one)."),
        para(state, "3. Copy the key (it starts with AIza) and paste it here."),
        btn(state, "Open Google AI Studio", false, Some(Message::Onboard(OnboardMsg::OpenUrl(AI_STUDIO_KEYS)))),
        row![
            text_input(if has_key { "••••••••" } else { "AIza…" }, &state.dj.key_input)
                .secure(true)
                .on_input(|v| Message::Setting(Setting::AiDjKeyInput(v)))
                .on_submit(Message::Setting(Setting::AiDjSaveKey))
                .style(style::input(t))
                .padding(Padding::from([8.0, 10.0]))
                .width(320),
            btn(
                state,
                "Save key",
                false,
                (!state.dj.key_input.trim().is_empty()).then_some(Message::Setting(Setting::AiDjSaveKey))
            ),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    ]
    .spacing(metrics::space_md());
    if let Some(status) = &state.dj.status {
        c = c.push(label(status.clone(), metrics::font_sm(), t.muted));
    }
    c.push(feature(
        state,
        "Turn on the AI DJ",
        "The free tier is enough for normal listening. Outside the EU, UK and Switzerland, Google may use what \
         Listen sends on the free tier (song titles and the DJ's lines) to improve its models.",
        state.config.settings.ai_dj.enabled,
        has_key.then_some(|v| Message::Setting(Setting::AiDj(v))),
    ))
}

/// One line per helper: installed or missing.
fn helper_rows(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let mut c = Column::new().spacing(metrics::space_sm());
    for h in &HELPERS {
        let ok = !state.helpers.missing.contains(&h.name);
        c = c.push(
            row![
                label_w(
                    if ok { "✓" } else { "✗" },
                    metrics::font_md(),
                    if ok { t.primary } else { t.error },
                    Weight::Bold
                ),
                label_w(h.name, metrics::font_md(), t.foreground, Weight::Semibold).width(90),
                label(crate::i18n::tr(h.what), metrics::font_sm(), t.muted),
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        );
    }
    c
}

fn helpers(state: &Listen) -> Column<'_, Message> {
    let t = state.tokens;
    let missing = !state.helpers.missing.is_empty();
    let mut c = column![
        label_w("Helper programs", 24.0, t.foreground, Weight::Bold),
        para(
            state,
            "Listen plays through mpv, and YouTube Music needs yt-dlp and Deno. They are separate open-source \
             programs; Listen installs the official packages with winget (Windows' package manager) in a window \
             of its own. mpv asks for administrator rights once."
        ),
        helper_rows(state),
        row![
            btn(state, "Install with winget", true, missing.then_some(Message::Helpers(HelpersMsg::Install))),
            btn(state, "Check again", false, Some(Message::Helpers(HelpersMsg::Check))),
        ]
        .spacing(metrics::space_sm()),
    ]
    .spacing(metrics::space_md());
    if state.helpers.started {
        c = c.push(para(
            state,
            "When winget is done, press “Check again”. Listen uses the programs after a restart.",
        ));
    }
    c
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let steps = Step::all();
    let at = steps.iter().position(|s| *s == state.onboard.step).unwrap_or(0);
    let mut nav = Column::new().spacing(metrics::space_xs()).width(200);
    for (i, step) in steps.iter().enumerate() {
        let active = i == at;
        nav = nav.push(
            button(label_w(
                format!("{}. {}", i + 1, crate::i18n::tr(step.label())),
                metrics::font_md(),
                if active { t.foreground } else { t.muted },
                Weight::Semibold,
            ))
            .width(Length::Fill)
            .padding(Padding::from([8.0, 12.0]))
            .style(style::nav_item(t, active))
            .on_press(Message::Onboard(OnboardMsg::Go(*step))),
        );
    }
    let content = match state.onboard.step {
        Step::Welcome => welcome(state),
        Step::Features => features(state),
        Step::Analysis => analysis(state),
        Step::AiDj => ai_dj(state),
        Step::Helpers => helpers(state),
    };
    let last = at + 1 == steps.len();
    let footer = row![
        btn(state, "Skip setup", false, Some(Message::Onboard(OnboardMsg::Finish))),
        space().width(Length::Fill),
        btn(state, "Back", false, (at > 0).then_some(Message::Onboard(OnboardMsg::Back))),
        btn(
            state,
            if last { "Connect your music" } else { "Next" },
            true,
            Some(Message::Onboard(OnboardMsg::Next))
        ),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let card = container(
        column![
            row![
                nav,
                scrollable(container(content).padding(Padding { right: 12.0, ..Padding::ZERO }))
                    .height(Length::Fill)
            ]
            .spacing(metrics::space_xl())
            .height(Length::Fill),
            footer,
        ]
        .spacing(metrics::space_lg()),
    )
    .padding(28)
    .max_width(960)
    .max_height(680)
    .style(move |_| container::Style {
        background: Some(t.surface.into()),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        ..Default::default()
    });
    container(card).center(Length::Fill).padding(24).into()
}

/// The helper-programs dialog (Windows; opens at start when something is missing).
pub fn helpers_modal(state: &Listen) -> Option<ModalParts<'_>> {
    let body = column![
        para(
            state,
            "Listen needs these programs to play music. It can install the official packages with winget \
             (Windows' package manager) in a window of its own; mpv asks for administrator rights once."
        ),
        helper_rows(state),
    ]
    .spacing(metrics::space_md());
    let actions = vec![
        btn(state, "Later", false, Some(Message::Helpers(HelpersMsg::Close))),
        btn(state, "Check again", false, Some(Message::Helpers(HelpersMsg::Check))),
        btn(
            state,
            "Install with winget",
            true,
            (!state.helpers.missing.is_empty()).then_some(Message::Helpers(HelpersMsg::Install)),
        ),
    ];
    Some(("Helper programs", body.into(), actions))
}

//! Advanced tab: updates, listening history, CD ripping, settings backup,
//! logging and caches.

use super::{line, section, stepper, switch};
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{button, column, pick_list, row, text_input};
use iced::{Alignment, Element, Padding};

pub(super) fn ripping(state: &Listen) -> Element<'_, Message> {
    use crate::app::rip::RipMsg;
    let t = state.tokens;
    let r = &state.config.settings.ripping;
    let small = |text: &'static str, m: Message, primary: bool| -> Element<'_, Message> {
        let b = button(label_w(
            text,
            metrics::font_md(),
            if primary { iced::Color::WHITE } else { t.foreground },
            Weight::Semibold,
        ))
        .padding(Padding::from([8.0, 16.0]))
        .on_press(m);
        if primary {
            b.style(style::primary_button(t)).into()
        } else {
            b.style(style::default_button(t)).into()
        }
    };
    let drive_line = match state.rip.drives.first() {
        Some(d) => {
            let offset = match listen_rip::whipper::resolve_offset(r.read_offset, d) {
                Some((o, src)) => format!("read offset {o:+} ({})", src.label()),
                None => "read offset unknown — enter it or use “Verify offset”".into(),
            };
            format!("{} at {} — {offset}", d.label(), d.device.display())
        }
        None => "No CD drive connected (a USB drive is picked up when it's plugged in).".into(),
    };
    let mut rows = vec![
        line(
            state,
            "Auto-rip inserted CDs",
            "Rip an audio CD as soon as it goes into the drive, also while Listen is in the tray. Always \
             FLAC (level 8), secure rip with AccurateRip (whipper), tagged from MusicBrainz.",
            switch(r.auto_rip, Setting::RipAuto),
        ),
        line(
            state,
            "Music folder",
            "Albums go to Album Artist/Album/NN Title.flac here — the folder Navidrome scans (the NAS \
             share, e.g. /mnt/nas/Music). Listen never deletes or overwrites anything in it.",
            row![
                text_input(crate::i18n::tr("/mnt/nas/Music"), &state.rip.dest_input)
                    .on_input(|v| Message::Setting(Setting::RipDestination(v)))
                    .on_submit(Message::Setting(Setting::RipSaveDestination))
                    .style(style::input(t))
                    .width(240),
                small("Save", Message::Setting(Setting::RipSaveDestination), false),
                small("Choose…", Message::Rip(RipMsg::BrowseDestination), false),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
        line(
            state,
            "Eject when done",
            "Eject the CD after ripping.",
            switch(r.eject_when_done, Setting::RipEject),
        ),
        line(
            state,
            "Notification sound",
            "Play a sound with the “Album ripped” notification.",
            switch(r.notify_sound, Setting::RipSound),
        ),
        line(
            state,
            "ReplayGain tags",
            "Measure track and album loudness while tagging.",
            switch(r.replay_gain, Setting::RipReplayGain),
        ),
        line(
            state,
            "Scan Navidrome afterwards",
            "Ask Navidrome to look for the new album (needs an admin login; otherwise its file watcher finds \
             it within a minute or two).",
            switch(r.scan_navidrome, Setting::RipScan),
        ),
        line(
            state,
            "Version picker",
            "When several releases match (e.g. K-pop versions), the best match is taken after this long.",
            stepper(
                state,
                5.0..=120.0,
                r.picker_timeout_secs as f32,
                format!("{} s", r.picker_timeout_secs),
                |v| Setting::RipPickerTimeout((v / 5.0).round() as u32 * 5),
            ),
        ),
        line(
            state,
            "Drive read offset",
            drive_line,
            row![
                text_input(crate::i18n::tr("auto"), &state.rip.offset_input)
                    .on_input(|v| Message::Setting(Setting::RipOffset(v)))
                    .on_submit(Message::Setting(Setting::RipSaveOffset))
                    .style(style::input(t))
                    .width(80),
                small("Save", Message::Setting(Setting::RipSaveOffset), false),
                button(label_w(
                    if state.rip.verifying_offset { "Checking…" } else { "Verify offset" },
                    metrics::font_md(),
                    t.foreground,
                    Weight::Semibold
                ))
                .padding(Padding::from([8.0, 16.0]))
                .style(style::default_button(t))
                .on_press_maybe(
                    (!state.rip.verifying_offset && !state.rip.drives.is_empty())
                        .then_some(Message::Setting(Setting::RipVerifyOffset))
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
    ];
    if state.rip_available() {
        rows.push(line(
            state,
            "Rip now",
            "Rip the CD that is in the drive (also one ripped before).",
            small("Rip CD", Message::Rip(RipMsg::RipNow(None)), true),
        ));
    }
    let to_fix: Vec<_> = state.rip.history.to_fix().collect();
    if !to_fix.is_empty() {
        rows.push(
            label_w("Rips to fix (not on MusicBrainz)", metrics::font_md(), t.foreground, Weight::Medium)
                .into(),
        );
        for e in to_fix.iter().rev().take(10) {
            rows.push(
                column![
                    label(e.folder.display().to_string(), metrics::font_sm(), t.foreground),
                    label(format!("Add the disc: {}", e.submit_url), metrics::font_xs(), t.muted),
                ]
                .spacing(2)
                .into(),
            );
        }
    }
    if let Some(status) = &state.rip.status {
        rows.push(label(status.clone(), metrics::font_md(), t.primary).into());
    }
    section(state, "CD ripping", rows)
}

pub(super) fn history(state: &Listen) -> Element<'_, Message> {
    use crate::app::history::HistoryMsg;
    use listen_core::settings::HistoryRetention;
    let t = state.tokens;
    let h = &state.config.settings.history;
    let small = |text: &'static str, msg: Message| {
        button(label_w(text, metrics::font_md(), t.foreground, Weight::Semibold))
            .padding(Padding::from([8.0, 16.0]))
            .style(style::default_button(t))
            .on_press(msg)
    };
    let clear: Element<'_, Message> = if state.history_view.confirm_clear {
        row![
            label("Delete everything?", metrics::font_md(), t.foreground),
            small("Yes, delete", Message::History(HistoryMsg::Clear)),
            small("No", Message::History(HistoryMsg::ConfirmClear(false))),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into()
    } else {
        small("Delete all", Message::History(HistoryMsg::ConfirmClear(true))).into()
    };
    let rows = vec![
        line(
            state,
            "Record listening history",
            "Saved only on this computer (song, server, time); never uploaded or shared. Off = nothing is written.",
            switch(h.enabled, Setting::HistoryEnabled),
        ),
        line(
            state,
            "Don't record this session",
            "Pauses recording until you close Listen.",
            switch(state.history_view.private, Setting::HistoryPrivate),
        ),
        line(
            state,
            "Keep history for",
            "Older entries are deleted automatically. Unlimited is needed for the year review.",
            pick_list(HistoryRetention::ALL, Some(h.retention), |r| {
                Message::Setting(Setting::HistoryRetention(r))
            })
            .into(),
        ),
        line(
            state,
            "Export",
            "Writes the history to your Downloads folder.",
            row![
                small("JSON", Message::History(HistoryMsg::Export(false))),
                small("CSV", Message::History(HistoryMsg::Export(true)))
            ]
            .spacing(metrics::space_sm())
            .into(),
        ),
        line(
            state,
            "Delete history",
            "Removes all entries; statistics and mixes are computed from them and disappear too. Single songs: right-click in Recently Played.",
            clear,
        ),
    ];
    section(state, "Listening history", rows)
}

fn small<'a>(state: &Listen, text: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(text, metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([8.0, 16.0]))
        .style(style::default_button(t))
        .on_press_maybe(msg)
        .into()
}

pub(super) fn updates(state: &Listen) -> Element<'_, Message> {
    use crate::app::advanced::AdvancedMsg;
    use crate::app::updater::{UpdateMsg, Version};
    use listen_core::settings::UpdateChannel;
    let a = &state.config.settings.advanced;
    let u = &state.adv.update;
    let status = match (&u.available, &u.status) {
        _ if u.checking => "Checking...".to_string(),
        (_, Some(s)) => s.clone(),
        _ => format!("You run Listen {}.", Version::current()),
    };
    let mut rows = vec![
        line(
            state,
            "Check for updates",
            status,
            small(
                state,
                if u.checking { "Checking..." } else { "Check now" },
                (!u.checking).then_some(Message::Update(UpdateMsg::CheckNow)),
            ),
        ),
        line(
            state,
            "Check on start",
            "Look for a new version when Listen starts. You decide whether to install it.",
            super::switch_with(a.update_check_on_start, |v| Message::Advanced(AdvancedMsg::CheckOnStart(v))),
        ),
        line(
            state,
            "Release channel",
            "Stable offers full releases only; Beta also offers pre-releases.",
            pick_list(UpdateChannel::ALL, Some(a.update_channel), |c| {
                Message::Advanced(AdvancedMsg::Channel(c))
            })
            .into(),
        ),
        if cfg!(windows) {
            line(
                state,
                "Helper programs",
                "mpv, yt-dlp and Deno, installed with winget. Update them when YouTube Music songs stop playing.",
                row![
                    small(state, "Show", Some(Message::Helpers(crate::app::helpers::HelpersMsg::Open))),
                    small(state, "Update", Some(Message::Helpers(crate::app::helpers::HelpersMsg::Update))),
                ]
                .spacing(metrics::space_sm())
                .into(),
            )
        } else {
            super::hidden()
        },
        line(
            state,
            "Release source",
            "Updates come from the public GitHub releases of leks2205/Listen, checked against their SHA256SUMS. No account needed.",
            small(state, "Open releases", Some(Message::Advanced(AdvancedMsg::OpenReleases))),
        ),
    ];
    if let Some(r) = &u.available {
        rows.insert(
            1,
            line(
                state,
                "Update available",
                format!("Listen {} is ready ({} channel).", r.version, a.update_channel),
                small(state, "View update", Some(Message::Update(UpdateMsg::Open))),
            ),
        );
    }
    section(state, "Updates", rows)
}

pub(super) fn backup(state: &Listen) -> Element<'_, Message> {
    use crate::app::advanced::AdvancedMsg;
    let t = state.tokens;
    let rows = vec![
        line(
            state,
            "Export settings",
            "Writes all settings to a JSON file in your Downloads folder. Passwords and tokens are not included.",
            small(state, "Export", Some(Message::Advanced(AdvancedMsg::ExportSettings))),
        ),
        line(
            state,
            "Import settings",
            "Reads a file made by \"Export settings\". Every value is checked; ones that don't fit are skipped and listed. \
             Replaces your current settings.",
            row![
                text_input(crate::i18n::tr("~/Downloads/listen-settings.json"), &state.adv.import_path)
                    .on_input(|v| Message::Advanced(AdvancedMsg::ImportPath(v)))
                    .on_submit(Message::Advanced(AdvancedMsg::ImportAsk))
                    .style(style::input(t))
                    .width(260),
                small(
                    state,
                    "Import",
                    (!state.adv.import_path.trim().is_empty())
                        .then_some(Message::Advanced(AdvancedMsg::ImportAsk)),
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
    ];
    section(state, "Settings backup", rows)
}

pub(super) fn logging(state: &Listen) -> Element<'_, Message> {
    use crate::app::advanced::AdvancedMsg;
    use listen_core::settings::LogLevel;
    let where_ =
        crate::logging::dir().map(|d| d.display().to_string()).unwrap_or_else(|| "(no log folder)".into());
    let rows = vec![
        line(
            state,
            "Log level",
            format!("How much listen.log records (in {where_}). Debug helps when something doesn't play."),
            pick_list(LogLevel::ALL, Some(state.config.settings.advanced.log_level), |l| {
                Message::Advanced(AdvancedMsg::LogLevel(l))
            })
            .into(),
        ),
        line(
            state,
            "Export diagnostics",
            "Saves a zip with the logs, your config and the program versions to Downloads. Passwords, cookies, \
             tokens and URL credentials are removed first.",
            small(
                state,
                if state.adv.diagnostics_busy { "Working..." } else { "Export" },
                (!state.adv.diagnostics_busy).then_some(Message::Advanced(AdvancedMsg::ExportDiagnostics)),
            ),
        ),
    ];
    section(state, "Logging", rows)
}

pub(super) fn cache(state: &Listen) -> Element<'_, Message> {
    use crate::app::advanced::{AdvancedMsg, CacheKind, format_bytes};
    let mut rows = vec![line(
        state,
        "Clear memory cache",
        "Soft clear: forgets the covers and data held in memory; they load again as you browse. Files on disk stay.",
        small(state, "Clear", Some(Message::Advanced(AdvancedMsg::ClearMemory))),
    )];
    for kind in CacheKind::ALL {
        let size = state.adv.cache_size(kind).map_or("measuring...".to_string(), format_bytes);
        let (name, hint): (&'static str, &'static str) = match kind {
            CacheKind::Covers => ("Cover art cache", "Downloaded album and artist images."),
            CacheKind::YtmAudio => ("YouTube Music audio cache", "Songs kept for gapless play and replays."),
            CacheKind::Waveforms => ("Waveform cache", "Waveforms of the player bar slider."),
        };
        rows.push(line(
            state,
            name,
            format!("{hint} {size} on disk."),
            small(state, "Clear", Some(Message::Advanced(AdvancedMsg::ClearAsk(Some(kind))))),
        ));
    }
    rows.push(line(
        state,
        "Clear all caches",
        "Hard clear: deletes every cache above. Your library, playlists and settings are not touched.",
        row![
            small(state, "Refresh sizes", Some(Message::Advanced(AdvancedMsg::RefreshCaches))),
            small(state, "Clear all", Some(Message::Advanced(AdvancedMsg::ClearAsk(None)))),
        ]
        .spacing(metrics::space_sm())
        .into(),
    ));
    section(state, "Cache", rows)
}

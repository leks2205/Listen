//! The Advanced tab: updater dialog and choices, settings export / import,
//! log level, caches. Nothing here reaches GitHub or installs anything: the
//! check results are fed in as messages.

use iced_test::simulator::Simulator;
use listen::app::advanced::{AdvancedMsg, CacheKind};
use listen::app::overlay::{Modal, OverlayMsg};
use listen::app::settings_shell::ShellMsg;
use listen::app::updater::{self, Phase, UpdateMsg};
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS};
use listen_core::settings::{LogLevel, SettingsTab, UpdateChannel};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = s.update(Message::Navigate(listen::app::Route::Settings));
    let _ = s.update(Message::Shell(ShellMsg::Tab(SettingsTab::Advanced)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3600.0), s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3600.0), s.view());
        ui.click(text).unwrap_or_else(|_| panic!("{text} is on screen"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

const RELEASES: &str = r###"[
  {"tag_name":"v99.1.0-beta.1","prerelease":true,"body":"beta","assets":[]},
  {"tag_name":"v99.0.0","prerelease":false,"body":"## Highlights\n- **Fast** updater\n- Dialog","assets":[]},
  {"tag_name":"v0.1.0","prerelease":false,"body":"old","assets":[]}
]"###;

fn checked(manual: bool, json: &str) -> Message {
    Message::Update(UpdateMsg::Checked { manual, result: updater::parse_releases(json) })
}

fn custom_open(s: &Listen) -> bool {
    matches!(s.overlay.modal, Some(Modal::Custom(k)) if k == updater::MODAL)
}

#[test]
fn tab_shows_the_new_sections() {
    let s = state();
    for text in [
        "Updates",
        "Check now",
        "Release channel",
        "Release source",
        "Settings backup",
        "Export settings",
        "Import settings",
        "Logging",
        "Log level",
        "Export diagnostics",
        "Cache",
        "Clear memory cache",
        "Cover art cache",
        "YouTube Music audio cache",
        "Clear all caches",
    ] {
        assert!(has_text(&s, text), "missing {text}");
    }
}

#[test]
fn a_found_update_opens_the_dialog_with_its_notes() {
    let mut s = state();
    let _ = s.update(checked(false, RELEASES));
    assert!(custom_open(&s));
    assert_eq!(s.adv.update.available.as_ref().unwrap().tag, "v99.0.0", "stable channel ignores the beta");
    for text in ["Skip this version", "Remind later", "Download and install"] {
        assert!(has_text(&s, text), "missing {text}");
    }
    let notes = listen::ui::update_dialog::plain_notes(&s.adv.update.available.as_ref().unwrap().body);
    assert_eq!(notes, "Highlights\n\u{2022} Fast updater\n\u{2022} Dialog");
    assert!(has_text(&s, &notes));
}

#[test]
fn download_and_install_never_installs_in_demo_mode() {
    let mut s = state();
    let _ = s.update(checked(true, RELEASES));
    click_text(&mut s, "Download and install");
    assert_eq!(s.adv.update.phase, Phase::Idle);
}

#[test]
fn skip_this_version_hides_automatic_dialogs_but_not_manual_checks() {
    let mut s = state();
    let _ = s.update(checked(false, RELEASES));
    click_text(&mut s, "Skip this version");
    assert!(!custom_open(&s));
    assert_eq!(s.config.settings.advanced.skipped_version, "v99.0.0");
    let _ = s.update(checked(false, RELEASES));
    assert!(!custom_open(&s), "automatic check respects the skip");
    let _ = s.update(checked(true, RELEASES));
    assert!(custom_open(&s), "Check now shows it anyway");
    // a newer version is not skipped
    let _ = s.update(Message::Overlay(OverlayMsg::CloseModal));
    let newer = RELEASES.replace("v99.0.0", "v99.2.0");
    let _ = s.update(checked(false, &newer));
    assert!(custom_open(&s));
}

#[test]
fn remind_later_waits_a_day() {
    let mut s = state();
    let _ = s.update(checked(false, RELEASES));
    click_text(&mut s, "Remind later");
    assert!(!custom_open(&s));
    let wait = s.config.settings.advanced.update_remind_after - listen_core::history::now_secs();
    assert!((23 * 3600..=24 * 3600).contains(&wait), "{wait}");
    let _ = s.update(checked(false, RELEASES));
    assert!(!custom_open(&s));
    assert!(s.adv.update.available.is_some(), "still offered in Settings");
    assert!(has_text(&s, "Update available"));
}

#[test]
fn channel_decides_whether_betas_are_offered() {
    let mut s = state();
    let _ = s.update(Message::Advanced(AdvancedMsg::Channel(UpdateChannel::Beta)));
    assert_eq!(s.config.settings.advanced.update_channel, UpdateChannel::Beta);
    let _ = s.update(checked(false, RELEASES));
    let r = s.adv.update.available.clone().unwrap();
    assert_eq!(r.tag, "v99.1.0-beta.1");
    assert!(r.prerelease);
    let line = format!(
        "Listen 99.1.0-beta.1 is available (you have {}). This is a pre-release.",
        updater::Version::current()
    );
    assert!(has_text(&s, &line));
}

#[test]
fn up_to_date_and_errors_are_reported() {
    let mut s = state();
    let _ = s.update(checked(true, r#"[{"tag_name":"v0.1.0","assets":[]}]"#));
    assert!(s.adv.update.available.is_none() && !custom_open(&s));
    assert!(s.overlay.toasts.iter().any(|t| t.message.contains("up to date")));
    let _ =
        s.update(Message::Update(UpdateMsg::Checked { manual: true, result: Err("GitHub says no".into()) }));
    assert!(s.overlay.toasts.iter().any(|t| t.message.contains("GitHub says no")));
    assert!(has_text(&s, "GitHub says no"));
}

#[test]
fn failed_install_shows_the_reason_and_allows_a_retry() {
    let mut s = state();
    let _ = s.update(checked(true, RELEASES));
    let _ = s.update(Message::Update(UpdateMsg::Installed(Err("checksum mismatch for x".into()))));
    assert!(has_text(&s, "Update failed: checksum mismatch for x") && has_text(&s, "Download and install"));
    let _ = s.update(Message::Update(UpdateMsg::Installed(Ok(updater::Installed::Local))));
    assert!(has_text(&s, "Restart now") && has_text(&s, "Later"));
}

#[test]
fn log_level_and_update_switches_persist_in_settings() {
    let mut s = state();
    assert_eq!(s.config.settings.advanced.log_level, LogLevel::Debug);
    let _ = s.update(Message::Advanced(AdvancedMsg::LogLevel(LogLevel::Info)));
    assert_eq!(s.config.settings.advanced.log_level, LogLevel::Info);
    let _ = s.update(Message::Advanced(AdvancedMsg::CheckOnStart(false)));
    assert!(!s.config.settings.advanced.update_check_on_start);
}

fn exported_path(s: &Listen) -> std::path::PathBuf {
    let msg =
        s.overlay.toasts.iter().find(|t| t.message.starts_with("Settings exported to")).expect("exported");
    std::path::PathBuf::from(msg.message.trim_start_matches("Settings exported to ").trim_end_matches('.'))
}

#[test]
fn settings_export_then_import_restores_them() {
    let mut s = state();
    s.config.settings.history.enabled = false;
    s.config.settings.advanced.update_channel = UpdateChannel::Beta;
    let _ = s.update(Message::Advanced(AdvancedMsg::ExportSettings));
    let path = exported_path(&s);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("listen-settings") && !text.to_lowercase().contains("password"));

    s.config.settings.history.enabled = true;
    s.config.settings.advanced.update_channel = UpdateChannel::Stable;
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportPath(path.display().to_string())));
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportAsk));
    assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))), "asks before replacing settings");
    assert!(s.config.settings.history.enabled, "nothing changed yet");
    let _ = s.update(Message::Overlay(OverlayMsg::Confirmed));
    assert!(!s.config.settings.history.enabled);
    assert_eq!(s.config.settings.advanced.update_channel, UpdateChannel::Beta);
    assert_eq!(s.config.settings.settings_tab, SettingsTab::Advanced, "stays on the tab");
    let _ = std::fs::remove_file(path);
}

#[test]
fn import_reports_bad_values_and_refuses_other_files() {
    let mut s = state();
    let dir = std::env::temp_dir().join(format!("listen-adv-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.json");
    std::fs::write(&bad, r#"{"history":{"enabled":"banana"},"advanced":{"log_level":"info"},"zzz":1}"#)
        .unwrap();
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportPath(bad.display().to_string())));
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportAsk));
    let Some(Modal::Confirm(c)) = &s.overlay.modal else { panic!("confirm expected") };
    assert!(c.text.contains("history.enabled") && c.text.contains("zzz"), "{}", c.text);
    let _ = s.update(Message::Overlay(OverlayMsg::Confirmed));
    assert_eq!(s.config.settings.advanced.log_level, LogLevel::Info);
    assert!(s.config.settings.history.enabled);

    let feishin = dir.join("feishin.json");
    std::fs::write(&feishin, r#"{"general":{},"playback":{}}"#).unwrap();
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportPath(feishin.display().to_string())));
    let _ = s.update(Message::Advanced(AdvancedMsg::ImportAsk));
    assert!(s.overlay.modal.is_none());
    assert!(s.overlay.toasts.iter().any(|t| t.message.contains("Feishin")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cache_sizes_are_shown_and_clearing_asks_first() {
    let mut s = state();
    assert!(has_text(&s, "Downloaded album and artist images. measuring... on disk."));
    let _ = s.update(Message::Advanced(AdvancedMsg::CacheSizes(vec![
        (CacheKind::Covers, 2_500_000),
        (CacheKind::YtmAudio, 1_200_000_000),
        (CacheKind::Waveforms, 0),
    ])));
    assert!(has_text(&s, "Downloaded album and artist images. 2.5 MB on disk."));
    assert!(has_text(&s, "Songs kept for gapless play and replays. 1.2 GB on disk."));
    assert!(has_text(&s, "Waveforms of the player bar slider. 0 B on disk."));
    let m = click_text(&mut s, "Clear all");
    assert!(!m.is_empty());
    assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))));
    let _ = s.update(Message::Overlay(OverlayMsg::CloseModal));
    let _ = s.update(Message::Advanced(AdvancedMsg::ClearMemory));
    assert!(s.overlay.toasts.iter().any(|t| t.message.contains("Memory cache cleared")));
}

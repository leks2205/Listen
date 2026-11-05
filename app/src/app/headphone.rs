//! Headphone correction (item 18): the AutoEq profile bound to the output that plays now is the
//! first stage of every deck's `af` chain (`listen_core::peq::mpv_filter`). With the audio device
//! on "auto" the system's default sink is the output (polled with `pactl get-default-sink` every
//! 0.5 s while a profile is bound), so plugging in or connecting headphones switches the profile
//! within a second. A switch rebuilds the decks' chains (not during a DJ blend; then right after).

use super::{Listen, Message};
use iced::Task;
use listen_core::peq::{self, Profile};
use std::time::Duration;

/// How often the default output is checked while a profile is bound (and without one, for the
/// settings page).
pub const POLL_BOUND: Duration = Duration::from_millis(500);
pub const POLL_IDLE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub enum HeadphoneMsg {
    Poll,
    /// The system's default output (sink name), `None` when it can't be told.
    DefaultSink(Option<String>),
    Enabled(bool),
    Query(String),
    /// Binds the index entry of this name to the output playing now.
    Use(String),
    /// Binds the `ParametricEQ.txt` text typed or pasted into the search box.
    UsePasted,
    /// Removes the profile of a device key.
    Remove(String),
}

#[derive(Debug, Default)]
pub struct HeadphoneState {
    /// The system's default sink (key form), when known.
    pub default_sink: Option<String>,
    polling: bool,
    /// The search box.
    pub query: String,
    /// The `af` stage the decks have now (a change rebuilds their chains).
    applied: String,
    /// A switch waits for the DJ blend to end.
    pending: bool,
}

/// The default sink's name (`pactl get-default-sink`; PipeWire and PulseAudio). Linux only.
fn default_sink() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let out = std::process::Command::new("pactl").arg("get-default-sink").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

impl Listen {
    /// The device key of the output that plays now: the chosen device, or on "auto" the system's
    /// default sink (when known).
    pub fn headphone_output(&self) -> String {
        let dev = &self.config.settings.playback.audio_device;
        if dev.trim().is_empty() || dev == "auto" {
            self.headphone.default_sink.clone().unwrap_or_else(|| "auto".into())
        } else {
            peq::device_key(dev).to_string()
        }
    }

    /// The profile in effect now, if any.
    pub fn headphone_profile(&self) -> Option<&Profile> {
        let p = &self.config.settings.playback;
        if !p.headphone_eq {
            return None;
        }
        p.headphone_profiles.get(&self.headphone_output()).or_else(|| p.headphone_profiles.get("auto"))
    }

    /// The headphone stage for the decks' chains ("" = none).
    pub(super) fn headphone_stage(&self) -> String {
        self.headphone_profile().map(peq::mpv_filter).unwrap_or_default()
    }

    /// A readable name for a device key (the device list's description when known).
    pub fn headphone_device_label(&self, key: &str) -> String {
        if key == "auto" {
            return crate::i18n::tr("System default output").to_string();
        }
        self.audio
            .devices
            .iter()
            .find(|(id, _)| peq::device_key(id) == key)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| key.to_string())
    }

    pub(super) fn headphone_poll_every(&self) -> Option<Duration> {
        let p = &self.config.settings.playback;
        if self.mock_playback() || !p.headphone_eq || !cfg!(target_os = "linux") {
            return None;
        }
        Some(if p.headphone_profiles.is_empty() { POLL_IDLE } else { POLL_BOUND })
    }

    /// Rebuilds the decks' chains when the stage changed (after a blend when one runs).
    fn headphone_reapply(&mut self) -> Task<Message> {
        let stage = self.headphone_stage();
        if stage == self.headphone.applied && !self.headphone.pending {
            return Task::none();
        }
        if self.mix.phase != super::mix::Phase::Idle {
            self.headphone.pending = true;
            return Task::none();
        }
        self.headphone.pending = false;
        let name = self.headphone_profile().map_or("none".to_string(), |p| p.name.clone());
        tracing::info!("headphone correction: output {} → {name}", self.headphone_output());
        self.headphone.applied = stage;
        self.apply_audio()
    }

    pub(super) fn on_headphone(&mut self, msg: HeadphoneMsg) -> Task<Message> {
        match msg {
            HeadphoneMsg::Poll => {
                if self.headphone.pending {
                    return self.headphone_reapply();
                }
                if self.headphone.polling {
                    return Task::none();
                }
                self.headphone.polling = true;
                Task::perform(async { tokio::task::spawn_blocking(default_sink).await.ok().flatten() }, |s| {
                    Message::Headphone(HeadphoneMsg::DefaultSink(s))
                })
            }
            HeadphoneMsg::DefaultSink(sink) => {
                self.headphone.polling = false;
                let sink = sink.map(|s| peq::device_key(&s).to_string());
                if sink == self.headphone.default_sink {
                    return Task::none();
                }
                self.headphone.default_sink = sink;
                self.headphone_reapply()
            }
            HeadphoneMsg::Enabled(v) => {
                self.config.settings.playback.headphone_eq = v;
                self.save_config();
                self.headphone_reapply()
            }
            HeadphoneMsg::Query(q) => {
                self.headphone.query = q;
                Task::none()
            }
            HeadphoneMsg::Use(name) => {
                let Some(e) = peq::index().iter().find(|e| e.name == name) else { return Task::none() };
                self.headphone_bind(e.profile.clone())
            }
            HeadphoneMsg::UsePasted => {
                let Some((preamp, filters)) = peq::parse_parametric_eq(&self.headphone.query) else {
                    return Task::none();
                };
                let name = crate::i18n::tr("Imported profile").to_string();
                self.headphone_bind(Profile { name, source: "imported".into(), preamp, filters })
            }
            HeadphoneMsg::Remove(key) => {
                self.config.settings.playback.headphone_profiles.remove(&key);
                self.save_config();
                self.headphone_reapply()
            }
        }
    }

    fn headphone_bind(&mut self, profile: Profile) -> Task<Message> {
        let key = self.headphone_output();
        let p = &mut self.config.settings.playback;
        p.headphone_profiles.insert(key, profile);
        p.headphone_eq = true;
        self.headphone.query.clear();
        self.save_config();
        self.headphone_reapply()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> Listen {
        let (mut s, _) = Listen::boot(&crate::Args { demo: true, ephemeral: true, ..crate::Args::default() });
        s.config.settings.playback.audio_device = "auto".into();
        s
    }

    #[test]
    fn the_default_sink_picks_its_profile() {
        let mut s = app();
        let xm5 = peq::search("WH-1000XM5", 1)[0].profile.clone();
        s.config.settings.playback.headphone_profiles.insert("bluez_output.AA_BB.1".into(), xm5.clone());
        assert!(s.headphone_profile().is_none(), "no default sink known");
        let _ = s.on_headphone(HeadphoneMsg::DefaultSink(Some("bluez_output.AA_BB.1".into())));
        assert_eq!(s.headphone_profile(), Some(&xm5));
        assert!(s.headphone_stage().starts_with("@hpeq:lavfi=["));
        assert_eq!(s.headphone.applied, s.headphone_stage(), "the decks were rebuilt");
        assert!(s.audio_settings_for(0).audio_filters.starts_with("@hpeq:"), "first in the chain");
        // Wired headphones: another sink, no profile → the stage goes.
        let _ = s.on_headphone(HeadphoneMsg::DefaultSink(Some(
            "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
        )));
        assert!(s.headphone_profile().is_none());
        assert!(!s.audio_settings_for(0).audio_filters.contains("hpeq"));
        // Off: never applied.
        let _ = s.on_headphone(HeadphoneMsg::DefaultSink(Some("bluez_output.AA_BB.1".into())));
        let _ = s.on_headphone(HeadphoneMsg::Enabled(false));
        assert!(s.headphone_profile().is_none());
    }

    #[test]
    fn a_chosen_device_is_the_output() {
        let mut s = app();
        s.config.settings.playback.audio_device = "pipewire/alsa_output.usb-dac".into();
        s.headphone.default_sink = Some("bluez_output.AA_BB.1".into());
        assert_eq!(s.headphone_output(), "alsa_output.usb-dac");
    }

    #[test]
    fn binding_uses_the_output_now_and_pasted_text_works() {
        let mut s = app();
        s.headphone.default_sink = Some("alsa_output.usb-dac".into());
        let _ = s.on_headphone(HeadphoneMsg::Use("Sennheiser HD 600".into()));
        assert_eq!(
            s.config.settings.playback.headphone_profiles["alsa_output.usb-dac"].name,
            "Sennheiser HD 600"
        );
        let _ = s.on_headphone(HeadphoneMsg::Query(
            "Preamp: -2 dB\nFilter 1: ON PK Fc 100 Hz Gain 2 dB Q 1".into(),
        ));
        let _ = s.on_headphone(HeadphoneMsg::UsePasted);
        let p = &s.config.settings.playback.headphone_profiles["alsa_output.usb-dac"];
        assert_eq!((p.source.as_str(), p.filters.len()), ("imported", 1));
        let _ = s.on_headphone(HeadphoneMsg::Remove("alsa_output.usb-dac".into()));
        assert!(s.config.settings.playback.headphone_profiles.is_empty());
    }
}

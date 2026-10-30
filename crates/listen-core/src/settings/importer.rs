//! Mapping a Feishin settings export onto Listen settings.

use super::*;

/// Applies a Feishin settings export on top of `base`. Unknown or missing
/// fields keep their current value; secrets (remote password) are ignored.
pub fn import_feishin(base: &Settings, json: &str) -> Result<Settings, serde_json::Error> {
    let v: Value = serde_json::from_str(json)?;
    let mut s = base.clone();
    let g = &v["general"];

    if let Some(tags) = v["queryBuilder"]["tag"].as_array() {
        s.query_builder.tags = tags
            .iter()
            .filter_map(|t| {
                Some(QueryTag {
                    label: t["label"].as_str().unwrap_or_default().to_string(),
                    kind: t["type"].as_str().unwrap_or("string").to_string(),
                    tag: t["value"].as_str().filter(|x| !x.is_empty())?.to_string(),
                })
            })
            .collect();
    }

    let a = &mut s.appearance;
    set_string(&mut a.theme, &g["theme"]);
    set_bool(&mut a.follow_system_theme, &g["followSystemTheme"]);
    set_string(&mut a.theme_dark, &g["themeDark"]);
    set_string(&mut a.theme_light, &g["themeLight"]);
    if let Some(accent) = g["accent"].as_str().and_then(Rgb::parse) {
        a.accent = Some(accent);
    }
    set_bool(&mut a.use_theme_accent, &g["useThemeAccentColor"]);
    set_string(&mut a.font, &v["font"]["builtIn"]);
    set_num(&mut a.zoom, &g["zoomFactor"]);
    set_num(&mut a.album_background_blur, &g["albumBackgroundBlur"]);
    set_num(&mut a.artist_background_blur, &g["artistBackgroundBlur"]);

    if let Some(items) = g["sidebarItems"].as_array() {
        s.sidebar.items = items
            .iter()
            .filter_map(|it| {
                Some(SidebarItem {
                    id: it["id"].as_str()?.to_string(),
                    label: it["label"].as_str().unwrap_or_default().to_string(),
                    visible: !it["disabled"].as_bool().unwrap_or(false),
                })
            })
            .collect();
    }
    set_bool(&mut s.sidebar.show_playlists, &g["sidebarPlaylistList"]);
    set_bool(&mut s.sidebar.playlist_folders, &g["sidebarPlaylistFolders"]);
    set_string(&mut s.sidebar.playlist_folder_separator, &g["sidebarPlaylistFolderSeparator"]);
    set_bool(&mut s.sidebar.show_image, &g["sidebarImageEnabled"]);
    set_bool(&mut s.sidebar.show_queue, &g["showQueueInSidebar"]);
    set_strings(&mut s.sidebar.panel_order, &g["sidebarPanelOrder"]);

    set_bool(&mut s.home.feature, &g["homeFeature"]);
    if let Some(items) = enabled_ids(&g["homeItems"]) {
        s.home.sections = items;
    }

    import_artist(&mut s.artist, g);

    let pb = &mut s.player_bar;
    if let Some(items) = enabled_ids(&g["playerItems"]) {
        pb.info_items = items;
    }
    set_bool(&mut pb.skip_buttons, &g["skipButtons"]["enabled"]);
    set_num(&mut pb.skip_backward_secs, &g["skipButtons"]["skipBackwardSeconds"]);
    set_num(&mut pb.skip_forward_secs, &g["skipButtons"]["skipForwardSeconds"]);
    set_num(&mut pb.volume_wheel_step, &g["volumeWheelStep"]);
    set_bool(&mut pb.open_drawer, &g["playerbarOpenDrawer"]);
    set_num(&mut s.fullscreen.auto_open_minutes, &g["fullscreenAutoOpenTimeout"]);
    set_num(&mut pb.volume_width, &g["volumeWidth"]);
    set_num(&mut pb.button_size, &g["buttonSize"]);

    let p = &v["playback"];
    let pl = &mut s.playback;
    let mpv = &p["mpvProperties"];
    set_string(&mut pl.gapless, &mpv["gaplessAudio"]);
    pl.replay_gain = match mpv["replayGainMode"].as_str() {
        Some("track") => ReplayGain::Track,
        Some("album") => ReplayGain::Album,
        Some(_) => ReplayGain::No,
        None => pl.replay_gain,
    };
    set_num(&mut pl.replay_gain_preamp_db, &mpv["replayGainPreampDB"]);
    // Feishin passes the mpv option replaygain-clip (yes = allow clipping); ours means "prevent clipping".
    if let Some(allow) = mpv["replayGainClip"].as_bool() {
        pl.replay_gain_clip = !allow;
    }
    set_string(&mut pl.audio_device, &p["mpvAudioDeviceId"]);
    set_strings(&mut pl.mpv_extra_args, &p["mpvExtraParameters"]);
    set_bool(&mut pl.fade_on_pause, &p["audioFadeOnStatusChange"]);
    set_bool(&mut pl.equalizer_enabled, &p["equalizer"]["enabled"]);
    set_num(&mut pl.equalizer_preamp, &p["equalizer"]["preamp"]);
    if let Some(bands) = p["equalizer"]["bands"].as_array() {
        pl.equalizer = bands
            .iter()
            .filter_map(|b| {
                Some(EqBand { freq: b["freq"].as_f64()? as f32, gain: b["gain"].as_f64()? as f32 })
            })
            .collect();
    }
    set_bool(&mut pl.preserve_pitch, &p["preservePitch"]);
    set_num(&mut pl.replay_gain_fallback_db, &mpv["replayGainFallbackDB"]);
    set_num(&mut pl.sample_rate_hz, &mpv["audioSampleRateHz"]);
    if let Some(mode) = mpv["audioExclusiveMode"].as_str() {
        pl.audio_exclusive = mode == "yes";
    }
    let comp = &p["compressor"];
    set_bool(&mut pl.compressor.enabled, &comp["enabled"]);
    set_num(&mut pl.compressor.params.threshold, &comp["threshold"]);
    set_num(&mut pl.compressor.params.ratio, &comp["ratio"]);
    set_num(&mut pl.compressor.params.attack, &comp["attack"]);
    set_num(&mut pl.compressor.params.release, &comp["release"]);
    set_num(&mut pl.compressor.params.makeup, &comp["makeup"]);
    set_num(&mut pl.compressor.params.knee, &comp["knee"]);
    let tc = &p["transcode"];
    set_bool(&mut pl.transcode.enabled, &tc["enabled"]);
    set_string(&mut pl.transcode.format, &tc["format"]);
    set_num(&mut pl.transcode.bitrate, &tc["bitrate"]);
    set_num(&mut pl.transcode.max_sample_rate, &tc["maxSampleRate"]);
    set_bool(&mut pl.scrobble, &p["scrobble"]["enabled"]);
    set_num(&mut pl.scrobble_at_percent, &p["scrobble"]["scrobbleAtPercentage"]);
    set_num(&mut pl.scrobble_at_secs, &p["scrobble"]["scrobbleAtDuration"]);

    let dj = &v["autoDJ"];
    set_bool(&mut s.auto_dj.enabled, &dj["enabled"]);
    set_num(&mut s.auto_dj.item_count, &dj["itemCount"]);
    set_num(&mut s.auto_dj.timing, &dj["timing"]);
    set_bool(&mut s.auto_dj.allow_duplicates, &dj["allowDuplicates"]);
    set_string(&mut s.auto_dj.strategy, &dj["songStrategy"]);

    let l = &v["lyrics"];
    set_bool(&mut s.lyrics.fetch, &l["fetch"]);
    set_bool(&mut s.lyrics.follow, &l["follow"]);
    set_string(&mut s.lyrics.alignment, &l["alignment"]);
    set_strings(&mut s.lyrics.sources, &l["sources"]);
    set_num(&mut s.lyrics.delay_ms, &l["delayMs"]);
    set_bool(&mut s.lyrics.prefer_local, &l["preferLocalLyrics"]);
    set_bool(&mut s.lyrics.show_provider, &l["showProvider"]);
    set_bool(&mut s.lyrics.show_match, &l["showMatch"]);
    set_bool(&mut s.lyrics.enable_romaji, &l["enableRomaji"]);
    set_bool(&mut s.lyrics.enable_furigana, &l["enableFurigana"]);
    set_bool(&mut s.lyrics.enable_netease_translation, &l["enableNeteaseTranslation"]);
    set_bool(&mut s.lyrics.enable_auto_translation, &l["enableAutoTranslation"]);
    set_string(&mut s.lyrics.translation_api_provider, &l["translationApiProvider"]);
    set_string(&mut s.lyrics.translation_api_key, &l["translationApiKey"]);
    set_string(&mut s.lyrics.translation_target_language, &l["translationTargetLanguage"]);
    set_num(&mut s.lyrics.follow_scroll_alignment, &l["followScrollAlignment"]);
    set_num(&mut s.lyrics.line_lead_time_ms, &l["lineLeadTimeMs"]);
    let ld = &v["lyricsDisplay"]["default"];
    set_num(&mut s.lyrics.font_size, &ld["fontSize"]);
    set_num(&mut s.lyrics.font_size_unsync, &ld["fontSizeUnsync"]);
    set_num(&mut s.lyrics.gap, &ld["gap"]);
    set_num(&mut s.lyrics.gap_unsync, &ld["gapUnsync"]);
    set_num(&mut s.lyrics.padding_left, &ld["paddingLeft"]);
    set_num(&mut s.lyrics.padding_right, &ld["paddingRight"]);
    set_num(&mut s.lyrics.scale_non_active, &ld["scaleNonActive"]);
    set_num(&mut s.lyrics.opacity_non_active, &ld["opacityNonActive"]);

    let am = &v["visualizer"]["audiomotionanalyzer"];
    if am.is_object() {
        let _ = s.visualizer.apply_json(&am.to_string());
        if let Some(presets) = am["presets"].as_array() {
            let builtin: Vec<String> = super::builtin_presets().into_iter().map(|p| p.name).collect();
            for p in presets {
                if let (Some(name), true) = (p["name"].as_str(), p["value"].is_object())
                    && !builtin.iter().any(|b| b == name)
                {
                    s.visualizer.presets.push(super::VisualizerPreset {
                        name: name.to_string(),
                        value: p["value"].to_string(),
                    });
                }
            }
        }
    }
    let ps = &v["general"]["playerbarSlider"];
    let pb = &mut s.player_bar;
    if let Some(t) = ps["type"].as_str() {
        pb.slider_type =
            if t == "waveform" { super::SliderType::Waveform } else { super::SliderType::Slider };
    }
    if let Some(a) = ps["barAlign"].as_str() {
        pb.waveform_align = match a {
            "top" => super::WaveformAlign::Top,
            "bottom" => super::WaveformAlign::Bottom,
            _ => super::WaveformAlign::Center,
        };
    }
    set_num(&mut pb.waveform_bar_width, &ps["barWidth"]);
    set_num(&mut pb.waveform_gap, &ps["barGap"]);
    set_num(&mut pb.waveform_radius, &ps["barRadius"]);
    set_bool(&mut pb.waveform_stretch, &ps["stretched"]);
    set_num(&mut pb.waveform_delay_secs, &ps["loadingDelay"]);

    if let Some(bindings) = v["hotkeys"]["bindings"].as_object() {
        s.hotkeys = bindings
            .iter()
            .filter_map(|(action, b)| {
                let key = b["hotkey"].as_str()?;
                (!key.is_empty()).then(|| (action.clone(), key.to_string()))
            })
            .collect();
        s.hotkeys_global = bindings
            .iter()
            .filter(|(action, b)| {
                b["isGlobal"].as_bool() == Some(true)
                    && b["hotkey"].as_str().is_some_and(|k| !k.is_empty())
                    && super::hotkey_action(action).is_some_and(|a| a.allow_global)
            })
            .map(|(action, _)| action.clone())
            .collect();
    }
    set_bool(&mut s.global_media_keys, &v["hotkeys"]["globalMediaHotkeys"]);

    let w = &v["window"];
    set_bool(&mut s.window.tray, &w["tray"]);
    set_bool(&mut s.window.minimize_to_tray, &w["minimizeToTray"]);
    set_bool(&mut s.window.exit_to_tray, &w["exitToTray"]);
    set_bool(&mut s.window.start_minimized, &w["startMinimized"]);
    set_bool(&mut s.window.discord_rpc, &v["discord"]["enabled"]);
    set_string(&mut s.window.discord_client_id, &v["discord"]["clientId"]);
    let dc = &v["discord"];
    let ig = &mut s.integrations;
    set_bool(&mut ig.discord.show_as_listening, &dc["showAsListening"]);
    set_bool(&mut ig.discord.show_paused, &dc["showPaused"]);
    set_bool(&mut ig.discord.state_icon, &dc["showStateIcon"]);
    set_bool(&mut ig.discord.server_image, &dc["showServerImage"]);
    match dc["displayType"].as_str() {
        Some("feishin") => ig.discord.display = DiscordDisplay::Feishin,
        Some("song") => ig.discord.display = DiscordDisplay::Song,
        Some("artist") => ig.discord.display = DiscordDisplay::Artist,
        _ => {}
    }
    match dc["linkType"].as_str() {
        Some("none") => ig.discord.link = DiscordLink::None,
        Some("last_fm") => ig.discord.link = DiscordLink::LastFm,
        Some("musicbrainz") => ig.discord.link = DiscordLink::MusicBrainz,
        Some("musicbrainz_last_fm") => ig.discord.link = DiscordLink::MusicBrainzLastFm,
        _ => {}
    }
    if let Some(mode) = p_scrobble_mode(&v["playback"]["scrobble"]["minimumMode"]) {
        ig.scrobble_mode = mode;
    }
    set_bool(&mut ig.notify_song_change, &v["playback"]["scrobble"]["notify"]);
    // The remote's password is never imported.
    set_bool(&mut ig.remote.enabled, &v["remote"]["enabled"]);
    set_num(&mut ig.remote.port, &v["remote"]["port"]);
    set_string(&mut ig.remote.username, &v["remote"]["username"]);
    set_bool(&mut s.window.title_bar_track_info, &w["windowBarTrackinfo"]);
    if let Some(style) = w["windowBarStyle"].as_str() {
        // Feishin: "windows" / "macos" draw their own bar; "native" uses the system one.
        s.window.custom_title_bar = style != "native";
    }

    Ok(s)
}

fn set_bool(dst: &mut bool, v: &Value) {
    if let Some(b) = v.as_bool() {
        *dst = b;
    }
}

fn set_string(dst: &mut String, v: &Value) {
    if let Some(s) = v.as_str() {
        *dst = s.to_string();
    }
}

fn set_strings(dst: &mut Vec<String>, v: &Value) {
    if let Some(arr) = v.as_array() {
        *dst = arr.iter().filter_map(|s| s.as_str().map(String::from)).collect();
    }
}

/// Sets any numeric field from a JSON number, ignoring out-of-range values.
fn set_num<T: TryFromF64>(dst: &mut T, v: &Value) {
    if let Some(n) = v.as_f64().and_then(T::try_from_f64) {
        *dst = n;
    }
}

/// The artist page settings (`artistItems`, `artistReleaseTypeItems`, `artistRadioCount`, external links).
fn import_artist(a: &mut ArtistSettings, g: &Value) {
    let items = |v: &Value| -> Option<Vec<SortableItem>> {
        let list: Vec<SortableItem> = v
            .as_array()?
            .iter()
            .filter_map(|it| {
                Some(SortableItem {
                    id: it["id"].as_str()?.to_string(),
                    enabled: it["disabled"].as_bool() != Some(true),
                })
            })
            .collect();
        (!list.is_empty()).then_some(list)
    };
    if let Some(list) = items(&g["artistItems"]) {
        a.items = list;
    }
    if let Some(list) = items(&g["artistReleaseTypeItems"]) {
        a.release_types = list;
        a.add_missing();
    }
    set_num(&mut a.radio_count, &g["artistRadioCount"]);
    let l = &mut a.external_links;
    set_bool(&mut l.enabled, &g["externalLinks"]);
    set_bool(&mut l.last_fm, &g["lastFM"]);
    set_bool(&mut l.music_brainz, &g["musicBrainz"]);
    set_bool(&mut l.listen_brainz, &g["listenBrainz"]);
    set_bool(&mut l.qobuz, &g["qobuz"]);
    set_bool(&mut l.spotify, &g["spotify"]);
    set_bool(&mut l.native_spotify, &g["nativeSpotify"]);
}

/// Ids of the enabled entries of a Feishin `[{id, disabled}]` list, deduplicated.
fn enabled_ids(v: &Value) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for it in v.as_array()? {
        if it["disabled"].as_bool() == Some(true) {
            continue;
        }
        if let Some(id) = it["id"].as_str().filter(|id| !out.iter().any(|o| o == id)) {
            out.push(id.to_string());
        }
    }
    Some(out)
}

trait TryFromF64: Sized {
    fn try_from_f64(v: f64) -> Option<Self>;
}

macro_rules! int_from_f64 {
    ($($t:ty),*) => {$(
        impl TryFromF64 for $t {
            fn try_from_f64(v: f64) -> Option<Self> {
                (v.fract() == 0.0 && v >= <$t>::MIN as f64 && v <= <$t>::MAX as f64).then_some(v as $t)
            }
        }
    )*};
}
int_from_f64!(u8, u16, u32, i32);

impl TryFromF64 for f32 {
    fn try_from_f64(v: f64) -> Option<Self> {
        v.is_finite().then_some(v as f32)
    }
}

fn p_scrobble_mode(v: &Value) -> Option<ScrobbleMode> {
    match v.as_str()? {
        "both" => Some(ScrobbleMode::Both),
        "percentage" => Some(ScrobbleMode::Percentage),
        "seconds" => Some(ScrobbleMode::Seconds),
        _ => None,
    }
}

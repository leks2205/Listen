//! Internet radio (`getInternetRadioStations` and friends) and `jukeboxControl`: request
//! parameters and answer parsing, both pure.

use listen_source::{InternetRadio, JukeboxCmd, JukeboxStatus};
use serde_json::Value;

/// The stations in a `getInternetRadioStations` answer's `internetRadioStations`.
pub fn stations(v: &Value) -> Vec<InternetRadio> {
    v["internetRadioStation"].as_array().map(|a| a.iter().filter_map(station).collect()).unwrap_or_default()
}

fn station(v: &Value) -> Option<InternetRadio> {
    let text = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    Some(InternetRadio {
        id: text("id")?,
        name: text("name").unwrap_or_default(),
        stream_url: text("streamUrl")?,
        home_page_url: text("homePageUrl").or_else(|| text("homepageUrl")),
        image_id: text("coverArt"),
    })
}

/// Parameters of `createInternetRadioStation`.
pub fn create_params(radio: &InternetRadio) -> Vec<(&'static str, String)> {
    let mut p = vec![("streamUrl", radio.stream_url.clone()), ("name", radio.name.clone())];
    if let Some(home) = &radio.home_page_url {
        p.push(("homepageUrl", home.clone()));
    }
    p
}

/// Parameters of `updateInternetRadioStation`; the homepage is always sent (empty clears it).
pub fn update_params(radio: &InternetRadio) -> Vec<(&'static str, String)> {
    vec![
        ("id", radio.id.clone()),
        ("streamUrl", radio.stream_url.clone()),
        ("name", radio.name.clone()),
        ("homepageUrl", radio.home_page_url.clone().unwrap_or_default()),
    ]
}

/// The parameters of a `jukeboxControl` call.
pub fn jukebox_params(cmd: &JukeboxCmd) -> Vec<(&'static str, String)> {
    let ids = |ids: &[String]| ids.iter().map(|i| ("id", i.clone())).collect::<Vec<_>>();
    match cmd {
        JukeboxCmd::Status => vec![("action", "status".into())],
        JukeboxCmd::Set(songs) => [vec![("action", "set".into())], ids(songs)].concat(),
        JukeboxCmd::Add(songs) => [vec![("action", "add".into())], ids(songs)].concat(),
        JukeboxCmd::Clear => vec![("action", "clear".into())],
        JukeboxCmd::Start => vec![("action", "start".into())],
        JukeboxCmd::Stop => vec![("action", "stop".into())],
        JukeboxCmd::Skip { index, offset_secs } => {
            vec![("action", "skip".into()), ("index", index.to_string()), ("offset", offset_secs.to_string())]
        }
        JukeboxCmd::SetGain(g) => {
            vec![("action", "setGain".into()), ("gain", format!("{:.2}", g.clamp(0.0, 1.0)))]
        }
    }
}

/// The status in a `jukeboxStatus` (or `jukeboxPlaylist`) object.
pub fn jukebox_status(v: &Value) -> JukeboxStatus {
    JukeboxStatus {
        current_index: v["currentIndex"].as_i64().unwrap_or(-1),
        playing: v["playing"].as_bool().unwrap_or(false),
        gain: v["gain"].as_f64().unwrap_or(1.0) as f32,
        position_secs: v["position"].as_u64().unwrap_or(0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_stations() {
        let v = json!({"internetRadioStation": [
            {"id": "1", "name": "Jazz", "streamUrl": "http://x/jazz", "homePageUrl": "http://x", "coverArt": "rd-1"},
            {"id": "2", "name": "Rock", "streamUrl": "http://x/rock"},
            {"id": "3", "name": "No stream"}
        ]});
        let s = stations(&v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].home_page_url.as_deref(), Some("http://x"));
        assert_eq!(s[1].home_page_url, None);
        assert_eq!(s[0].image_id.as_deref(), Some("rd-1"));
        assert_eq!(s[1].image_id, None);
        assert!(stations(&json!({})).is_empty());
    }

    #[test]
    fn builds_write_requests() {
        let r = InternetRadio {
            id: "7".into(),
            name: "N".into(),
            stream_url: "http://s".into(),
            home_page_url: None,
            image_id: None,
        };
        assert_eq!(create_params(&r), [("streamUrl", "http://s".into()), ("name", "N".into())]);
        let u = update_params(&r);
        assert_eq!(u[0], ("id", "7".into()));
        assert_eq!(u[3], ("homepageUrl", String::new()));
    }

    #[test]
    fn builds_jukebox_requests() {
        assert_eq!(jukebox_params(&JukeboxCmd::Status), [("action", "status".into())]);
        let set = jukebox_params(&JukeboxCmd::Set(vec!["a".into(), "b".into()]));
        assert_eq!(set, [("action", "set".into()), ("id", "a".into()), ("id", "b".into())]);
        let skip = jukebox_params(&JukeboxCmd::Skip { index: 2, offset_secs: 30 });
        assert_eq!(skip[1..], [("index", "2".into()), ("offset", "30".into())]);
        assert_eq!(jukebox_params(&JukeboxCmd::SetGain(1.7))[1], ("gain", "1.00".into()));
    }

    #[test]
    fn reads_the_status() {
        let s = jukebox_status(&json!({"currentIndex": 3, "playing": true, "gain": 0.5, "position": 12}));
        assert_eq!(s, JukeboxStatus { current_index: 3, playing: true, gain: 0.5, position_secs: 12 });
        assert_eq!(jukebox_status(&json!({})).current_index, -1);
    }
}

//! Song change notifications through the freedesktop notification service (as in Feishin: silent, cover as
//! icon, one second after the change).

use listen_core::model::Song;
use std::path::PathBuf;
use std::time::Duration;

/// A song change only notifies when it lasted this long (skipping through songs shows one).
pub const DEBOUNCE: Duration = Duration::from_secs(1);

/// What a notification shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub title: String,
    pub body: String,
    pub icon: Option<PathBuf>,
}

/// Title = the song, body = "artists · joined" and the album on the next line.
pub fn content(song: &Song, icon: Option<PathBuf>) -> Content {
    let artists = song.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" · ");
    Content {
        title: song.title.clone(),
        body: format!("{artists}\n{}", song.album.as_deref().unwrap_or("")),
        icon,
    }
}

/// Decides when a song change is shown: the latest song wins once it has lasted [`DEBOUNCE`]
/// (the caller waits, then asks [`Debounce::fire`]).
#[derive(Debug, Default)]
pub struct Debounce {
    seen: Option<u64>,
    shown: Option<u64>,
}

impl Debounce {
    /// The current entry (`None` = nothing queued). Returns the entry to wait
    /// a debounce for when it is a new one.
    pub fn changed(&mut self, current: Option<u64>) -> Option<u64> {
        if self.seen == current {
            return None;
        }
        self.seen = current;
        self.shown = None;
        current
    }

    /// The wait for `uid` is over: show it unless another song came in the
    /// meantime (or it was shown already).
    pub fn fire(&mut self, uid: u64) -> bool {
        if self.seen != Some(uid) || self.shown == Some(uid) {
            return false;
        }
        self.shown = Some(uid);
        true
    }
}

/// Sends the notification. Failures are only logged by the caller.
pub async fn send(c: &Content) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from(listen_core::config::APP_ID));
        hints.insert("suppress-sound", Value::Bool(true));
        hints.insert("urgency", Value::U8(0));
        hints.insert("category", Value::from("x-gnome.music"));
        let icon = c.icon.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let actions: Vec<&str> = Vec::new();
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Listen", 0u32, icon, c.title.as_str(), c.body.as_str(), actions, hints, 5000i32),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = c;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::ArtistRef;

    #[test]
    fn content_is_song_artists_and_album() {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.title = "Song".into();
        s.album = Some("Album".into());
        s.artists = vec![ArtistRef { id: None, name: "A".into() }, ArtistRef { id: None, name: "B".into() }];
        let c = content(&s, Some(PathBuf::from("/tmp/c.jpg")));
        assert_eq!(c.title, "Song");
        assert_eq!(c.body, "A · B\nAlbum");
        assert_eq!(c.icon, Some(PathBuf::from("/tmp/c.jpg")));
    }

    #[test]
    fn only_the_song_that_stays_is_shown() {
        let mut d = Debounce::default();
        // Skipping through 1, 2, 3 within the debounce: only 3 fires.
        assert_eq!(d.changed(Some(1)), Some(1));
        assert_eq!(d.changed(Some(1)), None);
        assert_eq!(d.changed(Some(2)), Some(2));
        assert_eq!(d.changed(Some(3)), Some(3));
        assert!(!d.fire(1));
        assert!(!d.fire(2));
        assert!(d.fire(3));
        // Once only; the same song again later (queue repeat) is a change.
        assert!(!d.fire(3));
        assert_eq!(d.changed(Some(4)), Some(4));
        assert_eq!(d.changed(Some(3)), Some(3));
        assert!(!d.fire(4));
        assert!(d.fire(3), "a new play of the same entry");
        assert!(!d.fire(3));
        // The queue ending cancels what waits.
        assert_eq!(d.changed(Some(5)), Some(5));
        assert_eq!(d.changed(None), None);
        assert!(!d.fire(5));
    }
}

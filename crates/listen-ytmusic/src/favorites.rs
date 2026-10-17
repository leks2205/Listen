//! Favourite albums and artists on YouTube Music, as InnerTube requests.
//!
//! * a **song** is liked with `like/like` (see `crate::YouTubeMusic::set_favorite`);
//! * an **album** is saved to the library by liking its audio playlist
//!   (`OLAK5uy_...`, ytmusicapi `rate_playlist`); `like/removelike` takes it out;
//! * an **artist** is subscribed to with `subscription/subscribe` and left with
//!   `subscription/unsubscribe`.
//!
//! Request builders are separate from sending, so tests can check them without an account.

use ytmapi_rs::common::{ArtistChannelID, LikeStatus, PlaylistID, YoutubeID};
use ytmapi_rs::query::{RatePlaylistQuery, SubscribeArtistQuery, UnsubscribeArtistsQuery};

/// Saves (`true`) or removes (`false`) the album behind `audio_playlist_id`.
pub fn album_library(audio_playlist_id: &str, saved: bool) -> RatePlaylistQuery<'static> {
    let status = if saved { LikeStatus::Liked } else { LikeStatus::Indifferent };
    RatePlaylistQuery::new(PlaylistID::from_raw(audio_playlist_id.to_string()), status)
}

pub fn subscribe(channel_id: &str) -> SubscribeArtistQuery<'static> {
    SubscribeArtistQuery::new(ArtistChannelID::from_raw(channel_id.to_string()))
}

pub fn unsubscribe(channel_ids: &[String]) -> UnsubscribeArtistsQuery<'static> {
    UnsubscribeArtistsQuery::new(channel_ids.iter().map(|c| ArtistChannelID::from_raw(c.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ytmapi_rs::query::PostQuery;

    #[test]
    fn saving_an_album_likes_its_audio_playlist() {
        let q = album_library("OLAK5uy_abc", true);
        assert_eq!(q.path(), "like/like");
        assert_eq!(serde_json::Value::Object(q.header()), json!({"target": {"playlistId": "OLAK5uy_abc"}}));
    }

    #[test]
    fn removing_an_album_removes_the_like() {
        let q = album_library("OLAK5uy_abc", false);
        assert_eq!(q.path(), "like/removelike");
        assert_eq!(serde_json::Value::Object(q.header()), json!({"target": {"playlistId": "OLAK5uy_abc"}}));
    }

    #[test]
    fn subscribing_names_the_channel() {
        let q = subscribe("UC123");
        assert_eq!(q.path(), "subscription/subscribe");
        assert_eq!(serde_json::Value::Object(q.header()), json!({"channelIds": ["UC123"]}));
    }

    #[test]
    fn unsubscribing_names_every_channel() {
        let q = unsubscribe(&["UC1".to_string(), "UC2".to_string()]);
        assert_eq!(q.path(), "subscription/unsubscribe");
        assert_eq!(serde_json::Value::Object(q.header()), json!({"channelIds": ["UC1", "UC2"]}));
    }
}

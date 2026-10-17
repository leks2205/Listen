//! Playlist writes: request parameters (pure) and Navidrome's native cover upload
//! (`POST /api/playlist/{id}/image`, Navidrome 0.61+).

use super::{Subsonic, Value};
use listen_source::{PlaylistUpdate, Result, SourceError};

/// Parameters of `updatePlaylist` for a metadata change: only the set fields (an empty comment clears it).
pub fn update_params(id: &str, update: &PlaylistUpdate) -> Vec<(&'static str, String)> {
    let mut p = vec![("playlistId", id.to_string())];
    if let Some(name) = &update.name {
        p.push(("name", name.clone()));
    }
    if let Some(comment) = &update.comment {
        p.push(("comment", comment.clone()));
    }
    if let Some(public) = update.public {
        p.push(("public", public.to_string()));
    }
    p
}

/// Parameters of `updatePlaylist` that remove the entries at `indices`.
pub fn remove_params(id: &str, indices: &[usize]) -> Vec<(&'static str, String)> {
    let mut p = vec![("playlistId", id.to_string())];
    p.extend(indices.iter().map(|i| ("songIndexToRemove", i.to_string())));
    p
}

/// `createPlaylist` parameters that replace playlist `id`'s songs with `songs`, in order (no "move").
pub fn replace_params(id: &str, songs: &[String]) -> Vec<(&'static str, String)> {
    let mut p = vec![("playlistId", id.to_string())];
    p.extend(songs.iter().map(|s| ("songId", s.clone())));
    p
}

/// `ids` in the order `order` names: `order[i]` is the index in `ids` of the
/// song that goes to position `i`. `None` when `order` is not a permutation.
pub fn apply_order(ids: &[String], order: &[usize]) -> Option<Vec<String>> {
    if order.len() != ids.len() {
        return None;
    }
    let mut seen = vec![false; ids.len()];
    for &i in order {
        if i >= ids.len() || std::mem::replace(&mut seen[i], true) {
            return None;
        }
    }
    Some(order.iter().map(|&i| ids[i].clone()).collect())
}

/// The body of a `multipart/form-data` request with one file field `image`.
pub fn multipart_image(boundary: &str, image: &[u8], mime: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(image.len() + 200);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"image\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(image);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl Subsonic {
    /// `POST` (with `image`) or `DELETE` on `/api/playlist/{id}/image`.
    async fn native_image(&self, id: &str, image: Option<(&[u8], &str)>) -> Result<()> {
        if !self.navidrome().await || !self.features().playlist_image {
            return Err(SourceError::Unsupported);
        }
        for attempt in 0..2 {
            let cached = self.native.token.lock().unwrap().clone();
            let token = match cached {
                Some(t) => t,
                None => self.native_login().await?,
            };
            let url = self
                .base
                .join(&format!("api/playlist/{id}/image"))
                .map_err(|e| SourceError::Other(e.to_string()))?;
            let req = match image {
                Some((bytes, mime)) => {
                    let boundary = format!("listen{:x}", rand::random::<u64>());
                    self.http
                        .post(url)
                        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                        .body(multipart_image(&boundary, bytes, mime))
                }
                None => self.http.delete(url),
            };
            let resp = req
                .header("x-nd-authorization", format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| SourceError::Network(e.without_url().to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.native.token.lock().unwrap() = None;
                continue;
            }
            return if resp.status().is_success() {
                Ok(())
            } else {
                Err(SourceError::Server {
                    code: resp.status().as_u16() as i32,
                    message: "playlist image".into(),
                })
            };
        }
        Err(SourceError::Auth("native login refused".into()))
    }

    pub(super) async fn upload_image(&self, id: &str, image: &[u8], mime: &str) -> Result<()> {
        self.native_image(id, Some((image, mime))).await
    }

    pub(super) async fn delete_image(&self, id: &str) -> Result<()> {
        self.native_image(id, None).await
    }

    /// `updatePlaylist` with `update`.
    pub(super) async fn update_metadata(&self, id: &str, update: &PlaylistUpdate) -> Result<()> {
        self.call::<Value>("updatePlaylist", "", &update_params(id, update)).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::ServerId;

    fn client() -> Subsonic {
        Subsonic::new(ServerId("t".into()), "https://music.example.com/nd", "alice", "secret").unwrap()
    }

    fn query(url: &url::Url) -> Vec<(String, String)> {
        url.query_pairs()
            .into_owned()
            .filter(|(k, _)| !matches!(k.as_str(), "u" | "t" | "s" | "v" | "c" | "f"))
            .collect()
    }

    #[test]
    fn update_sends_only_what_changed() {
        let update =
            PlaylistUpdate { name: Some("Road".into()), comment: Some(String::new()), public: Some(true) };
        let url = client().url("updatePlaylist", &update_params("pl1", &update));
        assert!(url.path().ends_with("/rest/updatePlaylist"));
        assert_eq!(
            query(&url),
            [("playlistId", "pl1"), ("name", "Road"), ("comment", ""), ("public", "true")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
        );
        let only_public = update_params("pl1", &PlaylistUpdate { public: Some(false), ..Default::default() });
        assert_eq!(only_public, [("playlistId", "pl1".to_string()), ("public", "false".to_string())]);
    }

    #[test]
    fn remove_names_every_index() {
        let p = remove_params("pl1", &[4, 0, 7]);
        let idx: Vec<_> =
            p.iter().filter(|(k, _)| *k == "songIndexToRemove").map(|(_, v)| v.as_str()).collect();
        assert_eq!(idx, ["4", "0", "7"]);
        assert_eq!(p[0], ("playlistId", "pl1".to_string()));
    }

    #[test]
    fn replace_keeps_the_new_order() {
        let ids: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let ordered = apply_order(&ids, &[2, 0, 1]).unwrap();
        assert_eq!(ordered, ["c", "a", "b"]);
        let p = replace_params("pl1", &ordered);
        let songs: Vec<_> = p.iter().filter(|(k, _)| *k == "songId").map(|(_, v)| v.as_str()).collect();
        assert_eq!(songs, ["c", "a", "b"]);
        assert_eq!(apply_order(&ids, &[0, 0, 1]), None);
        assert_eq!(apply_order(&ids, &[0, 1]), None);
        assert_eq!(apply_order(&ids, &[0, 1, 3]), None);
    }

    #[test]
    fn the_image_is_a_multipart_file_field() {
        let body = multipart_image("XX", &[1, 2, 3], "image/png");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--XX\r\nContent-Disposition: form-data; name=\"image\""));
        assert!(text.contains("Content-Type: image/png\r\n\r\n"));
        assert!(text.ends_with("\r\n--XX--\r\n"));
        assert!(body.windows(3).any(|w| w == [1, 2, 3]));
    }
}

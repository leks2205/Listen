//! The create / edit playlist dialog (Feishin `create-playlist-form.tsx`,
//! `update-playlist-form.tsx`): name, description and public, and for servers
//! that can store one a cover image.

use super::*;
use listen_source::{NewPlaylist, PlaylistUpdate};

/// An image picked for the cover, read from disk.
#[derive(Clone)]
pub struct PendingImage {
    pub name: String,
    pub mime: String,
    pub bytes: Arc<Vec<u8>>,
}

impl std::fmt::Debug for PendingImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PendingImage({}, {}, {} B)", self.name, self.mime, self.bytes.len())
    }
}

#[derive(Debug, Clone)]
pub enum FormKind {
    /// A new playlist on `server`, starting with `songs`.
    Create {
        server: ServerId,
        songs: Vec<Song>,
    },
    Edit(Playlist),
}

/// The dialog's state.
#[derive(Debug, Clone)]
pub struct Form {
    pub kind: FormKind,
    pub name: String,
    pub comment: String,
    pub public: bool,
    pub image: Option<PendingImage>,
    /// Delete the uploaded cover on save.
    pub remove_image: bool,
    pub busy: bool,
    pub source: SourceKind,
    pub caps: Capabilities,
}

impl Form {
    /// Description and public flag can be stored.
    pub fn has_details(&self) -> bool {
        self.caps.contains(Capabilities::PLAYLIST_UPDATE)
    }

    /// A cover image can be uploaded (editing only).
    pub fn has_image(&self) -> bool {
        matches!(self.kind, FormKind::Edit(_)) && self.caps.contains(Capabilities::PLAYLIST_IMAGE)
    }

    pub fn is_create(&self) -> bool {
        matches!(self.kind, FormKind::Create { .. })
    }

    /// The change an edit makes: only fields that differ from the playlist.
    pub fn update(&self) -> PlaylistUpdate {
        let FormKind::Edit(p) = &self.kind else { return PlaylistUpdate::default() };
        let name = self.name.trim();
        PlaylistUpdate {
            name: (name != p.name && !name.is_empty()).then(|| name.to_string()),
            comment: self
                .has_details()
                .then_some(self.comment.trim())
                .filter(|c| *c != p.description.as_deref().unwrap_or_default().trim())
                .map(str::to_string),
            public: (self.has_details() && self.public != p.public).then_some(self.public),
        }
    }

    pub fn submittable(&self) -> bool {
        !self.busy && !self.name.trim().is_empty()
    }
}

/// What a finished dialog changed.
#[derive(Debug, Clone)]
pub enum FormDone {
    Created(Playlist),
    Updated(Playlist),
}

/// The type of an image file by its extension.
pub fn image_mime(path: &std::path::Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// The biggest cover Listen uploads.
const MAX_IMAGE: u64 = 10 * 1024 * 1024;

/// Native "open file" dialog (see [`crate::dialog`]) and the picked image.
async fn pick_image() -> Option<PendingImage> {
    let picked = crate::dialog::open_file(crate::dialog::Pick {
        title: "Choose a cover image",
        start: listen_core::platform::home_dir().map(|h| h.join("Pictures")),
        filter_name: "Images",
        patterns: &["*.png", "*.jpg", "*.jpeg", "*.webp", "*.gif"],
    })
    .await;
    let path = picked?;
    let mime = image_mime(&path)?;
    let meta = tokio::fs::metadata(&path).await.ok()?;
    if meta.len() > MAX_IMAGE {
        return None;
    }
    let bytes = tokio::fs::read(&path).await.ok()?;
    Some(PendingImage {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        mime: mime.into(),
        bytes: Arc::new(bytes),
    })
}

impl Listen {
    /// The capabilities and kind of the source of `server`.
    fn server_caps(&self, server: &ServerId) -> (Capabilities, SourceKind) {
        if self.demo {
            let ytm = server.0 == "yt";
            let p = demo::navidrome_playlist().playlist;
            let kind = if ytm { SourceKind::YouTubeMusic } else { SourceKind::Subsonic };
            return (self.playlist_caps(&Playlist { source: kind, ..p }), kind);
        }
        match self.playlist_source(server) {
            Some(s) => (s.capabilities(), s.kind()),
            None => (Capabilities::empty(), SourceKind::Subsonic),
        }
    }

    /// Opens the create dialog (Feishin "Create playlist").
    fn open_create_form(&mut self, songs: Vec<Song>) {
        let server = songs
            .first()
            .map(|s| s.server_id.clone())
            .or_else(|| self.config.active_server.clone())
            .unwrap_or_else(|| ServerId(String::new()));
        let (caps, source) = self.server_caps(&server);
        if !caps.contains(Capabilities::PLAYLIST_CREATE) {
            self.toast_info("This server cannot create playlists.".into());
            return;
        }
        self.playlist_page.form = Some(Form {
            kind: FormKind::Create { server, songs },
            name: String::new(),
            comment: String::new(),
            public: false,
            image: None,
            remove_image: false,
            busy: false,
            source,
            caps,
        });
        self.open_modal(FORM_MODAL);
    }

    /// Opens the edit dialog for `p`.
    fn open_edit_form(&mut self, p: Playlist) {
        let (caps, source) = self.server_caps(&p.server_id);
        if !caps.intersects(Capabilities::PLAYLIST_UPDATE | Capabilities::PLAYLIST_RENAME) {
            self.toast_info("This server cannot change playlists.".into());
            return;
        }
        // The list entry may lack the description the page has.
        let known = self
            .library
            .playlist
            .as_ref()
            .filter(|d| d.playlist.id == p.id)
            .map(|d| d.playlist.clone())
            .unwrap_or(p);
        self.playlist_page.form = Some(Form {
            name: known.name.clone(),
            comment: known.description.clone().unwrap_or_default(),
            public: known.public,
            kind: FormKind::Edit(known),
            image: None,
            remove_image: false,
            busy: false,
            source,
            caps,
        });
        self.open_modal(FORM_MODAL);
    }

    pub(super) fn on_playlist_form(&mut self, msg: PlaylistPageMsg) -> Task<Message> {
        use PlaylistPageMsg as P;
        match msg {
            P::Create(songs) => self.open_create_form(songs),
            P::EditPlaylist(p) => self.open_edit_form(p),
            P::FormName(v) => self.with_form(|f| f.name = v),
            P::FormComment(v) => self.with_form(|f| f.comment = v),
            P::FormPublic(v) => self.with_form(|f| f.public = v),
            P::FormRemoveImage => self.with_form(|f| {
                f.remove_image = !f.remove_image;
                f.image = None;
            }),
            P::FormPickImage => {
                return Task::perform(pick_image(), |i| Message::PlaylistPage(P::FormImagePicked(i)));
            }
            P::FormImagePicked(image) => match image {
                Some(image) => self.with_form(|f| {
                    f.image = Some(image);
                    f.remove_image = false;
                }),
                None => self.toast_info("No image chosen (PNG, JPEG, WebP or GIF up to 10 MB).".into()),
            },
            P::CoverPick => {
                return Task::perform(pick_image(), |i| Message::PlaylistPage(P::CoverPicked(i)));
            }
            P::CoverPicked(Some(image)) => return self.cover_change(Some(image)),
            P::CoverPicked(None) => {
                self.toast_info("No image chosen (PNG, JPEG, WebP or GIF up to 10 MB).".into());
            }
            P::CoverDelete => return self.cover_change(None),
            P::CoverDone(Ok(text)) => {
                self.toast_success(text);
                // The cover changed on the server: read the playlist and the list again.
                let reload = self.playlist_page.id.clone().map(|id| self.open_playlist(id));
                return Task::batch([self.load_playlists(), reload.unwrap_or_else(Task::none)]);
            }
            P::CoverDone(Err(e)) => self.toast_error(e),
            P::FormSubmit => return self.submit_form(),
            P::FormDone(result) => return self.form_done(result),
            _ => {}
        }
        Task::none()
    }

    /// Uploads `image` as the open playlist's cover, or deletes the uploaded cover.
    fn cover_change(&mut self, image: Option<PendingImage>) -> Task<Message> {
        let Some(detail) = self.open_playlist_detail() else { return Task::none() };
        let (id, server) = (detail.playlist.id.clone(), detail.playlist.server_id.clone());
        let done = |r| Message::PlaylistPage(PlaylistPageMsg::CoverDone(r));
        let Some(source) = self.playlist_source(&server).filter(|_| !self.demo) else {
            return Task::done(done(Ok("Cover changed (demo).".into())));
        };
        Task::perform(
            async move {
                match image {
                    Some(image) => source
                        .upload_playlist_image(&id, &image.bytes, &image.mime)
                        .await
                        .map(|_| "Cover uploaded.".to_string())
                        .map_err(|e| format!("Could not upload the cover: {e}")),
                    None => source
                        .delete_playlist_image(&id)
                        .await
                        .map(|_| "Cover removed.".to_string())
                        .map_err(|e| format!("Could not remove the cover: {e}")),
                }
            },
            done,
        )
    }

    fn with_form(&mut self, f: impl FnOnce(&mut Form)) {
        if let Some(form) = self.playlist_page.form.as_mut().filter(|f| !f.busy) {
            f(form);
        }
    }

    fn submit_form(&mut self) -> Task<Message> {
        let Some(form) = self.playlist_page.form.as_mut().filter(|f| f.submittable()) else {
            return Task::none();
        };
        form.busy = true;
        let form = form.clone();
        let done = |r| Message::PlaylistPage(PlaylistPageMsg::FormDone(r));
        match &form.kind {
            FormKind::Create { server, songs } => {
                let new = NewPlaylist {
                    name: form.name.trim().to_string(),
                    comment: if form.has_details() { form.comment.trim().to_string() } else { String::new() },
                    public: form.has_details() && form.public,
                };
                let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
                if self.demo {
                    let mut p = demo::navidrome_playlist().playlist;
                    p.id = format!("demo-new-{}", self.library.playlists.len());
                    p.name = new.name.clone();
                    p.description = Some(new.comment.clone()).filter(|c| !c.is_empty());
                    p.public = new.public;
                    p.song_count = Some(ids.len() as u32);
                    p.image = None;
                    return Task::done(done(Ok(FormDone::Created(p))));
                }
                let Some(source) = self.playlist_source(server) else {
                    return Task::done(done(Err("not connected".into())));
                };
                Task::perform(
                    async move {
                        source.create_playlist_full(&new, &ids).await.map(FormDone::Created).map_err(err)
                    },
                    done,
                )
            }
            FormKind::Edit(p) => {
                let update = form.update();
                let mut done_playlist = p.clone();
                if let Some(n) = &update.name {
                    done_playlist.name = n.clone();
                }
                if let Some(c) = &update.comment {
                    done_playlist.description = Some(c.clone()).filter(|c| !c.is_empty());
                }
                if let Some(v) = update.public {
                    done_playlist.public = v;
                }
                if self.demo {
                    return Task::done(done(Ok(FormDone::Updated(done_playlist))));
                }
                let Some(source) = self.playlist_source(&p.server_id) else {
                    return Task::done(done(Err("not connected".into())));
                };
                let id = p.id.clone();
                let (image, remove_image) = (form.image.clone(), form.remove_image);
                let metadata = form.caps.contains(Capabilities::PLAYLIST_UPDATE);
                Task::perform(
                    async move {
                        if metadata {
                            source.update_playlist(&id, &update).await.map_err(err)?;
                        } else if let Some(name) = &update.name {
                            source.rename_playlist(&id, name).await.map_err(err)?;
                        }
                        if let Some(image) = image {
                            source
                                .upload_playlist_image(&id, &image.bytes, &image.mime)
                                .await
                                .map_err(err)?;
                        } else if remove_image {
                            source.delete_playlist_image(&id).await.map_err(err)?;
                        }
                        Ok(FormDone::Updated(done_playlist))
                    },
                    done,
                )
            }
        }
    }

    fn form_done(&mut self, result: Result<FormDone, String>) -> Task<Message> {
        let creating = self.playlist_page.form.as_ref().is_some_and(Form::is_create);
        match result {
            Err(e) => {
                if let Some(f) = self.playlist_page.form.as_mut() {
                    f.busy = false;
                }
                self.toast_error(format!(
                    "Could not {} the playlist: {e}",
                    if creating { "create" } else { "change" }
                ));
                Task::none()
            }
            Ok(FormDone::Created(p)) => {
                self.playlist_page.form = None;
                self.close_modal();
                self.toast_success(format!("Created “{}”.", p.name));
                self.playlist_page.recent.insert(p.server_id.clone(), p.id.clone());
                if self.demo {
                    self.library.playlists.push(p);
                    self.playlists_list_sync();
                    return Task::none();
                }
                Task::batch([self.load_playlists()])
            }
            Ok(FormDone::Updated(p)) => {
                self.playlist_page.form = None;
                self.close_modal();
                self.toast_success("Playlist updated.".into());
                let patch = |q: &mut Playlist| {
                    q.name = p.name.clone();
                    q.description = p.description.clone();
                    q.public = p.public;
                };
                self.library.playlists.iter_mut().filter(|q| q.id == p.id).for_each(patch);
                if let Some(d) = self.library.playlist.as_mut().filter(|d| d.playlist.id == p.id) {
                    patch(&mut d.playlist);
                }
                self.playlists_list_sync();
                if self.demo { Task::none() } else { self.load_playlists() }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist() -> Playlist {
        let mut p = crate::demo::navidrome_playlist().playlist;
        p.name = "Road".into();
        p.description = Some("old".into());
        p.public = false;
        p
    }

    fn form(caps: Capabilities) -> Form {
        let p = playlist();
        Form {
            name: p.name.clone(),
            comment: "old".into(),
            public: false,
            kind: FormKind::Edit(p),
            image: None,
            remove_image: false,
            busy: false,
            source: SourceKind::Subsonic,
            caps,
        }
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let mut f = form(Capabilities::PLAYLIST_UPDATE);
        assert_eq!(f.update(), PlaylistUpdate::default());
        f.name = " Road trip ".into();
        f.public = true;
        assert_eq!(
            f.update(),
            PlaylistUpdate { name: Some("Road trip".into()), comment: None, public: Some(true) }
        );
        // A cleared description is sent as empty.
        f.comment = "  ".into();
        assert_eq!(f.update().comment, Some(String::new()));
    }

    #[test]
    fn a_server_without_details_edits_the_name_only() {
        let mut f = form(Capabilities::PLAYLIST_RENAME);
        f.comment = "new".into();
        f.public = true;
        f.name = "New".into();
        assert_eq!(f.update(), PlaylistUpdate { name: Some("New".into()), ..Default::default() });
        assert!(!f.has_details());
    }

    #[test]
    fn needs_a_name_and_images_need_the_capability() {
        let mut f = form(Capabilities::PLAYLIST_UPDATE | Capabilities::PLAYLIST_IMAGE);
        assert!(f.submittable() && f.has_image());
        f.name = "  ".into();
        assert!(!f.submittable());
        assert!(!form(Capabilities::PLAYLIST_UPDATE).has_image());
    }

    #[test]
    fn image_types_follow_the_extension() {
        let m = |p: &str| image_mime(std::path::Path::new(p));
        assert_eq!(m("/a/b.PNG"), Some("image/png"));
        assert_eq!(m("c.jpeg"), Some("image/jpeg"));
        assert_eq!(m("c.txt"), None);
        assert_eq!(m("noext"), None);
    }
}

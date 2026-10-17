//! Saved list queries ("collections", Feishin `save-as-collection-button.tsx`):
//! a named sort, order, search and filter set for the Albums or Tracks list.

use serde::{Deserialize, Serialize};

/// Which list a collection is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CollectionList {
    #[default]
    Albums,
    Songs,
}

/// A named query. Sorts are stored by `AlbumSort` / `SongSort` variant name, so new variants don't break it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub name: String,
    pub list: CollectionList,
    pub sort: String,
    pub descending: bool,
    pub search: String,
    pub favorite: Option<bool>,
    pub rated: Option<bool>,
    pub compilation: Option<bool>,
    pub recently_played: bool,
    pub genres: Vec<String>,
    /// (id, name) of the picked artists.
    pub artists: Vec<(String, String)>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    /// Custom tag filters: tag name and the picked value ids.
    pub tags: Vec<(String, Vec<String>)>,
}

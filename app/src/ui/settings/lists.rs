//! General tab: what the Play buttons do and how grids select (Feishin
//! "Play button behavior", "Grid multi-select").

use super::{line, section};
use crate::app::library_extra::ExtraMsg;
use crate::app::list::ListMsg;
use crate::app::{Listen, Message};
use iced::Element;
use iced::widget::{pick_list, toggler};
use listen_core::settings::PlayBehavior;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice(PlayBehavior);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label())
    }
}

pub(super) fn lists(state: &Listen) -> Element<'_, Message> {
    let l = &state.config.settings.lists;
    section(
        state,
        "Lists",
        vec![
            line(
                state,
                "Play button behavior",
                "What the Play buttons, a double-click on a song and the play hotkey do. Shuffle play and the DJs are not affected",
                pick_list(PlayBehavior::ALL.map(Choice), Some(Choice(l.play_behavior)), |c| {
                    Message::List(ListMsg::PlayBehavior(c.0))
                })
                .into(),
            ),
            line(
                state,
                "Grid multi-select",
                "Clicking a card's cover selects it. When off, it opens the item instead",
                toggler(l.grid_multi_select)
                    .on_toggle(|v| Message::List(ListMsg::GridMultiSelect(v)))
                    .size(20)
                    .into(),
            ),
            line(
                state,
                "Show favourites",
                "Show the heart buttons and favourite columns everywhere",
                toggler(l.show_favorites)
                    .on_toggle(|v| Message::Extra(ExtraMsg::ShowFavorites(v)))
                    .size(20)
                    .into(),
            ),
            line(
                state,
                "Show ratings",
                "Show star ratings (Navidrome / OpenSubsonic) on tracks, albums and artists",
                toggler(l.show_ratings)
                    .on_toggle(|v| Message::Extra(ExtraMsg::ShowRatings(v)))
                    .size(20)
                    .into(),
            ),
        ],
    )
}

//! The About tab: sourced facts about the playing song and its credits (`app::about`).

use crate::app::about::AboutMsg;
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::{data_label, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, scrollable, text};
use iced::{Element, Length, Padding};
use listen_lyrics::card::Card;

fn centred<'a>(state: &'a Listen, msg: String) -> Element<'a, Message> {
    let t = state.tokens;
    container(text(crate::i18n::tr(&msg).to_string()).size(metrics::font_lg()).color(t.muted).center())
        .padding(metrics::space_lg())
        .center(Length::Fill)
        .into()
}

/// A small link button ("wikipedia.org"); `name` is a page title or a search query, shown as it is.
fn link<'a>(state: &'a Listen, name: String, url: String) -> Element<'a, Message> {
    let t = state.tokens;
    button(data_label(name, metrics::font_xs(), t.primary))
        .padding(Padding::from([2.0, 8.0]))
        .style(style::bare_button(t))
        .on_press(Message::About(AboutMsg::Open(url)))
        .into()
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let a = &state.about;
    if state.queue.current().is_none() {
        return centred(state, "Nothing is playing.".into());
    }
    if a.loading {
        return centred(state, "Looking up this song…".into());
    }
    let Some(card) = a.card.as_ref().filter(|c| !c.is_empty()) else {
        let msg = if let Some(e) = &a.error {
            crate::i18n::trf("Could not look up this song: {}", &[e])
        } else if state.demo || !state.about_available() {
            "About this song needs the AI DJ's Gemini key (Settings → AI DJ).".into()
        } else if a.card.is_some() {
            "Nothing with a source was found about this song.".into()
        } else {
            String::new()
        };
        return centred(state, msg);
    };
    card_view(state, card)
}

fn card_view<'a>(state: &'a Listen, card: &'a Card) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new().spacing(metrics::space_md()).padding(Padding::from([8.0, 16.0]));
    col = col.push(label_w("About this song", metrics::font_lg(), t.foreground, Weight::Bold));
    for f in &card.facts {
        let mut sources = Row::new().spacing(metrics::space_xs());
        for s in &f.sources {
            sources = sources.push(link(state, s.title.clone(), s.url.clone()));
        }
        col = col.push(
            column![text(f.text.as_str()).size(metrics::font_md()).color(t.foreground), sources.wrap(),]
                .spacing(2),
        );
    }
    if !card.queries.is_empty() {
        let mut row =
            Row::new().spacing(metrics::space_xs()).push(label("Searched for:", metrics::font_xs(), t.muted));
        for q in &card.queries {
            let mut u = url::Url::parse("https://www.google.com/search").expect("static url");
            u.query_pairs_mut().append_pair("q", q);
            row = row.push(link(state, q.clone(), u.into()));
        }
        col = col.push(row.wrap());
    }
    if let (false, Some(url)) = (card.credits.credits.is_empty(), card.credits.url.as_ref()) {
        col = col.push(label_w("Credits", metrics::font_md(), t.foreground, Weight::Bold));
        for c in &card.credits.credits {
            col = col.push(
                column![
                    label(crate::i18n::tr(&c.role).to_string(), metrics::font_xs(), t.muted),
                    text(c.names.join(", ")).size(metrics::font_md()).color(t.foreground),
                ]
                .spacing(2),
            );
        }
        col = col.push(link(state, "musicbrainz.org".into(), url.clone()));
    }
    col = col.push(
        button(label("Look it up again", metrics::font_xs(), t.muted))
            .padding(Padding::from([4.0, 8.0]))
            .style(style::bare_button(t))
            .on_press(Message::About(AboutMsg::Refresh)),
    );
    col = col.push(label(
        "Facts by Gemini with Google Search; each links the pages it rests on. Credits from MusicBrainz.",
        metrics::font_xs(),
        t.muted,
    ));
    scrollable(col).style(style::scrollbar(t)).height(Length::Fill).into()
}

//! The full-screen player's background: the cover's dominant colour, the
//! blurred cover over it (optional) and a black veil of `opacity / 120`.

use crate::app::{Listen, Message};
use iced::widget::{container, image, space, stack};
use iced::{Background, Color, Element, Length};

/// `c` pulled towards the theme so its text stays readable: dark themes cap
/// the brightness, light themes floor it.
pub fn fit_to_theme(c: Color, dark: bool) -> Color {
    let lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if dark {
        const MAX: f32 = 0.32;
        if lum > MAX {
            let k = MAX / lum;
            return Color::from_rgb(c.r * k, c.g * k, c.b * k);
        }
    } else {
        const MIN: f32 = 0.72;
        if lum < MIN {
            let k = (MIN - lum) / (1.0 - lum);
            return Color::from_rgb(c.r + (1.0 - c.r) * k, c.g + (1.0 - c.g) * k, c.b + (1.0 - c.b) * k);
        }
    }
    c
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings.fullscreen;
    let fill = |c: Color| -> Element<'_, Message> {
        container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(Background::Color(c)), ..Default::default() })
            .into()
    };
    if !s.dynamic_background {
        return fill(t.background);
    }
    let base = state
        .fs
        .backdrop
        .as_ref()
        .map_or_else(|| crate::style::darken(t.primary, 0.55), |b| fit_to_theme(b.color, t.dark));
    let mut layers = stack![fill(base)];
    if s.dynamic_is_image
        && let Some(handle) = state.fs.backdrop.as_ref().and_then(|b| b.image.clone())
    {
        layers = layers.push(
            container(
                image(handle).width(Length::Fill).height(Length::Fill).content_fit(iced::ContentFit::Cover),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true),
        );
    }
    layers
        .push(fill(Color { a: s.overlay_alpha(), ..Color::BLACK }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color) -> f32 {
        0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
    }

    #[test]
    fn bright_covers_are_darkened_on_dark_themes() {
        assert!(lum(fit_to_theme(Color::WHITE, true)) <= 0.33);
        let dark = Color::from_rgb(0.1, 0.1, 0.2);
        assert_eq!(fit_to_theme(dark, true), dark);
    }

    #[test]
    fn dark_covers_are_lightened_on_light_themes() {
        assert!(lum(fit_to_theme(Color::BLACK, false)) >= 0.71);
        assert_eq!(fit_to_theme(Color::WHITE, false), Color::WHITE);
    }
}

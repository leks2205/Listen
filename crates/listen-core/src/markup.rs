//! Plain text for receivers that parse markup: desktop notification servers that advertise
//! `body-markup` (Desktop Notifications spec). Every notification sender (app, CD ripper) goes
//! through [`notification_body`]; song titles like "Simon & Garfunkel" or "<3" are otherwise cut off
//! or hidden, and a title could inject markup.

use std::borrow::Cow;

/// `s` with `& < > " '` escaped as XML entities.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// The notification body to send: escaped when the server advertises `body-markup` (`markup`),
/// as is for servers that show plain text.
pub fn notification_body(body: &str, markup: bool) -> Cow<'_, str> {
    if markup { escape(body).into() } else { body.into() }
}

#[cfg(test)]
mod tests {
    use super::notification_body;

    #[test]
    fn the_body_is_escaped_only_for_servers_that_read_markup() {
        let body = "Simon & Garfunkel\n<Intro> \"Live\" '73";
        assert_eq!(
            notification_body(body, true),
            "Simon &amp; Garfunkel\n&lt;Intro&gt; &quot;Live&quot; &apos;73"
        );
        assert_eq!(notification_body(body, false), body);
    }
}

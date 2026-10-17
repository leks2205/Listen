//! SOAP request building and response parsing for UPnP control, and the
//! DIDL-Lite metadata and time formats AVTransport uses.

use std::collections::HashMap;
use std::time::Duration;

/// A UPnP error answer or an unreadable one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SoapError {
    #[error("the device refused: {description} (UPnP error {code})")]
    Fault { code: u32, description: String },
    #[error("unreadable answer: {0}")]
    Malformed(String),
}

/// Escapes text for an XML element or attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// The SOAP envelope of one action. Argument values are escaped here.
pub fn envelope(service_type: &str, action: &str, args: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (name, value) in args {
        body.push_str(&format!("<{name}>{}</{name}>", escape(value)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
<s:Body><u:{action} xmlns:u=\"{service_type}\">{body}</u:{action}></s:Body></s:Envelope>"
    )
}

/// The `SOAPACTION` header value (with its quotes).
pub fn action_header(service_type: &str, action: &str) -> String {
    format!("\"{service_type}#{action}\"")
}

/// The out-arguments of an action response, or the UPnP fault it carries.
pub fn parse_response(xml: &str) -> Result<HashMap<String, String>, SoapError> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| SoapError::Malformed(e.to_string()))?;
    let body = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Body")
        .ok_or_else(|| SoapError::Malformed("no <Body>".into()))?;
    let first = body
        .children()
        .find(|n| n.is_element())
        .ok_or_else(|| SoapError::Malformed("empty <Body>".into()))?;
    if first.tag_name().name() == "Fault" {
        let find = |name: &str| {
            first
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == name)
                .and_then(|n| n.text())
                .map(|t| t.trim().to_string())
        };
        return Err(SoapError::Fault {
            code: find("errorCode").and_then(|c| c.parse().ok()).unwrap_or(500),
            description: find("errorDescription").or_else(|| find("faultstring")).unwrap_or_default(),
        });
    }
    Ok(first
        .children()
        .filter(|n| n.is_element())
        .map(|n| (n.tag_name().name().to_string(), n.text().unwrap_or("").trim().to_string()))
        .collect())
}

/// "H:MM:SS" (AVTransport's REL_TIME / duration format).
pub fn format_time(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Parses "H:MM:SS[.mmm]", with the optional sign and "+" some renderers add.
/// `NOT_IMPLEMENTED` and the like give `None`.
pub fn parse_time(text: &str) -> Option<Duration> {
    let text = text.trim().trim_start_matches('+');
    let mut parts = text.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let secs: f64 = s.split('/').next()?.parse().ok()?;
    let total = h.parse::<u64>().ok()? as f64 * 3600.0 + m.parse::<u64>().ok()? as f64 * 60.0 + secs;
    (total.is_finite() && total >= 0.0).then(|| Duration::from_secs_f64(total))
}

/// What a renderer shows about the track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub duration: Option<Duration>,
}

/// The DIDL-Lite document for `SetAVTransportURI`'s metadata argument (not
/// yet escaped for embedding).
pub fn didl_lite(meta: &TrackMeta, uri: &str, mime: &str) -> String {
    let mut out = String::from(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">",
    );
    out.push_str("<item id=\"0\" parentID=\"0\" restricted=\"1\">");
    out.push_str(&format!("<dc:title>{}</dc:title>", escape(&meta.title)));
    if !meta.artist.is_empty() {
        out.push_str(&format!(
            "<upnp:artist>{}</upnp:artist><dc:creator>{}</dc:creator>",
            escape(&meta.artist),
            escape(&meta.artist)
        ));
    }
    if !meta.album.is_empty() {
        out.push_str(&format!("<upnp:album>{}</upnp:album>", escape(&meta.album)));
    }
    if let Some(art) = &meta.art_url {
        out.push_str(&format!("<upnp:albumArtURI>{}</upnp:albumArtURI>", escape(art)));
    }
    out.push_str("<upnp:class>object.item.audioItem.musicTrack</upnp:class>");
    let duration = meta.duration.map(|d| format!(" duration=\"{}\"", format_time(d))).unwrap_or_default();
    out.push_str(&format!(
        "<res protocolInfo=\"http-get:*:{}:DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000\"{duration}>{}</res>",
        escape(mime),
        escape(uri)
    ));
    out.push_str("</item></DIDL-Lite>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::description::AV_TRANSPORT;

    #[test]
    fn builds_a_set_uri_envelope_with_escaped_arguments() {
        let xml = envelope(
            AV_TRANSPORT,
            "SetAVTransportURI",
            &[
                ("InstanceID", "0"),
                ("CurrentURI", "http://h/a?b=1&c=2"),
                ("CurrentURIMetaData", "<DIDL-Lite/>"),
            ],
        );
        assert!(xml.contains("<u:SetAVTransportURI xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"));
        assert!(xml.contains("<InstanceID>0</InstanceID>"));
        assert!(xml.contains("<CurrentURI>http://h/a?b=1&amp;c=2</CurrentURI>"));
        assert!(xml.contains("<CurrentURIMetaData>&lt;DIDL-Lite/&gt;</CurrentURIMetaData>"));
        assert!(roxmltree::Document::parse(&xml).is_ok(), "the envelope is well-formed XML");
        assert_eq!(
            action_header(AV_TRANSPORT, "Play"),
            "\"urn:schemas-upnp-org:service:AVTransport:1#Play\""
        );
    }

    #[test]
    fn parses_out_arguments() {
        let xml = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>
<u:GetPositionInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
<Track>1</Track><TrackDuration>0:03:25</TrackDuration><TrackURI>http://h/a.mp3</TrackURI><RelTime>0:01:02.500</RelTime>
</u:GetPositionInfoResponse></s:Body></s:Envelope>"#;
        let r = parse_response(xml).unwrap();
        assert_eq!(r["TrackDuration"], "0:03:25");
        assert_eq!(r["RelTime"], "0:01:02.500");
        assert_eq!(r["TrackURI"], "http://h/a.mp3");
    }

    #[test]
    fn parses_upnp_faults() {
        let xml = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault>
<faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring>
<detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>701</errorCode><errorDescription>Transition not available</errorDescription></UPnPError></detail>
</s:Fault></s:Body></s:Envelope>"#;
        assert_eq!(
            parse_response(xml).unwrap_err(),
            SoapError::Fault { code: 701, description: "Transition not available".into() }
        );
        assert!(matches!(parse_response("garbage"), Err(SoapError::Malformed(_))));
    }

    #[test]
    fn times_round_trip_and_odd_ones_are_none() {
        assert_eq!(format_time(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(parse_time("1:02:05"), Some(Duration::from_secs(3725)));
        assert_eq!(parse_time("00:00:07.500"), Some(Duration::from_millis(7500)));
        assert_eq!(parse_time("NOT_IMPLEMENTED"), None);
        assert_eq!(parse_time("-0:00:01"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn didl_carries_the_metadata_escaped() {
        let meta = TrackMeta {
            title: "Rock & Roll <Live>".into(),
            artist: "AC/DC".into(),
            album: "Album".into(),
            art_url: Some("http://h/art?a=1&b=2".into()),
            duration: Some(Duration::from_secs(205)),
        };
        let didl = didl_lite(&meta, "http://h/s/1.mp3", "audio/mpeg");
        assert!(roxmltree::Document::parse(&didl).is_ok());
        assert!(didl.contains("<dc:title>Rock &amp; Roll &lt;Live&gt;</dc:title>"));
        assert!(didl.contains("duration=\"0:03:25\""));
        assert!(didl.contains("http-get:*:audio/mpeg:"));
        assert!(didl.contains(">http://h/s/1.mp3</res>"));
        assert!(didl.contains("<upnp:class>object.item.audioItem.musicTrack</upnp:class>"));
    }
}

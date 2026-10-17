//! SSDP discovery: one M-SEARCH for media renderers and the parsing of the
//! unicast answers.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::Instant;

pub const MEDIA_RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
pub const MULTICAST: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);

/// One answer to an M-SEARCH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpResponse {
    /// URL of the device description.
    pub location: String,
    /// The search target the device answers for.
    pub st: String,
    pub usn: String,
}

/// The M-SEARCH datagram. `mx` is the seconds devices may wait before answering.
pub fn search_request(st: &str, mx: u8) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: {mx}\r\nST: {st}\r\nUSER-AGENT: Listen/1 UPnP/1.1\r\n\r\n"
    )
}

/// Parses an `HTTP/1.1 200 OK` answer. `None` for anything else (including
/// the devices' own NOTIFY datagrams and answers without a LOCATION).
pub fn parse_response(datagram: &str) -> Option<SsdpResponse> {
    let mut lines = datagram.lines();
    let status = lines.next()?;
    let mut parts = status.split_whitespace();
    if !parts.next()?.starts_with("HTTP/1.") || parts.next()? != "200" {
        return None;
    }
    let (mut location, mut st, mut usn) = (None, String::new(), String::new());
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "location" => location = Some(value.to_string()),
            "st" => st = value.to_string(),
            "usn" => usn = value.to_string(),
            _ => {}
        }
    }
    let location = location.filter(|l| l.starts_with("http://") || l.starts_with("https://"))?;
    Some(SsdpResponse { location, st, usn })
}

/// Sends the M-SEARCH to `target` (the multicast group, or a test responder)
/// and collects answers for `wait`. Answers are de-duplicated by LOCATION.
pub async fn search(target: SocketAddr, st: &str, wait: Duration) -> std::io::Result<Vec<SsdpResponse>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    let _ = socket.set_multicast_ttl_v4(2);
    let request = search_request(st, wait.as_secs().clamp(1, 5) as u8);
    // UDP is lossy: ask twice.
    socket.send_to(request.as_bytes(), target).await?;
    let deadline = Instant::now() + wait;
    let mut resend = Some(Instant::now() + Duration::from_millis(250));
    let mut found: Vec<SsdpResponse> = Vec::new();
    let mut buf = vec![0u8; 4096];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if resend.is_some_and(|at| now >= at) {
            resend = None;
            let _ = socket.send_to(request.as_bytes(), target).await;
        }
        let slice = resend.map_or(deadline, |at| at.min(deadline));
        match tokio::time::timeout_at(slice, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _from))) => {
                if let Some(r) = std::str::from_utf8(&buf[..n]).ok().and_then(parse_response)
                    && !found.iter().any(|f| f.location == r.location)
                {
                    found.push(r);
                }
            }
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: http://192.168.1.40:49152/description.xml\r\nSERVER: Linux/5 UPnP/1.0 Test/1\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:1234::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";

    #[test]
    fn request_names_the_target_and_man_header() {
        let r = search_request(MEDIA_RENDERER, 2);
        assert!(r.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(r.contains("MAN: \"ssdp:discover\"\r\n"));
        assert!(r.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n"));
        assert!(r.contains("MX: 2\r\n"));
        assert!(r.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_an_answer_with_any_header_case() {
        let r = parse_response(ANSWER).unwrap();
        assert_eq!(r.location, "http://192.168.1.40:49152/description.xml");
        assert_eq!(r.st, MEDIA_RENDERER);
        assert!(r.usn.starts_with("uuid:1234"));
        let lower = ANSWER.replace("LOCATION", "location").replace("ST:", "st:");
        assert_eq!(parse_response(&lower).unwrap().location, r.location);
    }

    #[test]
    fn rejects_notify_errors_and_answers_without_location() {
        assert!(parse_response("NOTIFY * HTTP/1.1\r\nLOCATION: http://x/\r\n\r\n").is_none());
        assert!(parse_response("HTTP/1.1 500 Oops\r\nLOCATION: http://x/\r\n\r\n").is_none());
        assert!(parse_response("HTTP/1.1 200 OK\r\nST: x\r\n\r\n").is_none());
        assert!(parse_response("HTTP/1.1 200 OK\r\nLOCATION: ftp://x/\r\n\r\n").is_none());
        assert!(parse_response("").is_none());
    }

    #[tokio::test]
    async fn search_collects_answers_from_a_responder() {
        let responder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = responder.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok((n, from)) = responder.recv_from(&mut buf).await {
                if std::str::from_utf8(&buf[..n]).is_ok_and(|s| s.starts_with("M-SEARCH")) {
                    let _ = responder.send_to(ANSWER.as_bytes(), from).await;
                }
            }
        });
        let found = search(addr, MEDIA_RENDERER, Duration::from_millis(700)).await.unwrap();
        assert_eq!(found.len(), 1, "the same device answering twice counts once");
        assert_eq!(found[0].location, "http://192.168.1.40:49152/description.xml");
    }
}

//! LAN autodiscovery (Feishin: `main/features/core/autodiscover`): a UDP
//! broadcast "who is JellyfinServer?" on port 7359; every server answers with
//! `{"Address":"http://host:8096","Id":"…","Name":"…"}`. Navidrome and other
//! Subsonic servers have no such protocol.

use serde::Deserialize;
use std::net::SocketAddr;
use std::time::Duration;

pub const PORT: u16 = 7359;
pub const PROBE: &[u8] = b"who is JellyfinServer?";

/// A server that answered the broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub url: String,
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Reply {
    address: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

/// Reads a server's answer; anything else on the port is ignored.
pub fn parse_reply(packet: &[u8]) -> Option<Found> {
    let reply: Reply = serde_json::from_slice(packet).ok()?;
    let url = reply.address.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let name = if reply.name.trim().is_empty() { url.clone() } else { reply.name.trim().to_string() };
    Some(Found { name, url, id: reply.id })
}

/// Sends the probe to each target and collects the answers for `wait`
/// (the same server answering twice is listed once).
pub async fn discover_at(targets: &[SocketAddr], wait: Duration) -> Vec<Found> {
    let Ok(socket) = tokio::net::UdpSocket::bind(("0.0.0.0", 0)).await else { return Vec::new() };
    let _ = socket.set_broadcast(true);
    for target in targets {
        let _ = socket.send_to(PROBE, target).await;
    }
    let mut found: Vec<Found> = Vec::new();
    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + wait;
    while let Ok(Ok((n, _))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
        if let Some(server) = parse_reply(&buf[..n])
            && !found.iter().any(|f| f.url == server.url)
        {
            found.push(server);
        }
    }
    found
}

/// Broadcasts on the loopback and default route (so a server on this
/// machine is found too) and waits three seconds, like Feishin.
pub async fn discover() -> Vec<Found> {
    let targets: [SocketAddr; 2] = [([127, 255, 255, 255], PORT).into(), ([255, 255, 255, 255], PORT).into()];
    discover_at(&targets, Duration::from_secs(3)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_server_reply() {
        let packet = br#"{"Address":"http://192.168.1.5:8096/","Id":"abc","Name":"Living room","EndpointAddress":null}"#;
        assert_eq!(
            parse_reply(packet),
            Some(Found {
                name: "Living room".into(),
                url: "http://192.168.1.5:8096".into(),
                id: "abc".into()
            })
        );
    }

    #[test]
    fn ignores_spurious_packets() {
        assert_eq!(parse_reply(b"who is JellyfinServer?"), None);
        assert_eq!(parse_reply(br#"{"Address":"ftp://x"}"#), None);
        assert_eq!(parse_reply(br#"{"Id":"x"}"#), None);
        let nameless = parse_reply(br#"{"Address":"https://jf.example.com"}"#).unwrap();
        assert_eq!(nameless.name, "https://jf.example.com");
    }

    #[tokio::test]
    async fn finds_answering_servers_once() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], PROBE);
            let reply = br#"{"Address":"http://10.0.0.2:8096","Id":"1","Name":"NAS"}"#;
            server.send_to(b"noise", from).await.unwrap();
            server.send_to(reply, from).await.unwrap();
            server.send_to(reply, from).await.unwrap();
        });
        let found = discover_at(&[addr], Duration::from_millis(400)).await;
        assert_eq!(
            found,
            vec![Found { name: "NAS".into(), url: "http://10.0.0.2:8096".into(), id: "1".into() }]
        );
    }
}

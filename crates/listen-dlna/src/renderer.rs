//! A media renderer on the LAN: discovery and the AVTransport /
//! RenderingControl actions Listen needs.

use crate::description::{self, AV_TRANSPORT, RENDERING_CONTROL};
use crate::soap::{self, TrackMeta};
use crate::{Error, Result, ssdp};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;
use url::Url;

/// What `GetTransportInfo` says the renderer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Playing,
    Paused,
    Stopped,
    Transitioning,
    NoMedia,
    Other,
}

impl TransportState {
    pub fn parse(text: &str) -> Self {
        match text.trim().to_ascii_uppercase().as_str() {
            "PLAYING" => Self::Playing,
            "PAUSED_PLAYBACK" | "PAUSED_RECORDING" | "PAUSED" => Self::Paused,
            "STOPPED" => Self::Stopped,
            "TRANSITIONING" => Self::Transitioning,
            "NO_MEDIA_PRESENT" => Self::NoMedia,
            _ => Self::Other,
        }
    }
}

/// `GetPositionInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionInfo {
    pub position: Option<Duration>,
    pub duration: Option<Duration>,
    pub uri: String,
}

/// A discovered MediaRenderer. Cheap to clone (the HTTP client is shared).
#[derive(Debug, Clone)]
pub struct Renderer {
    pub name: String,
    pub udn: String,
    pub model: String,
    av_control: Url,
    rc_control: Option<Url>,
    http: reqwest::Client,
}

impl PartialEq for Renderer {
    fn eq(&self, other: &Self) -> bool {
        self.udn == other.udn && self.av_control == other.av_control
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default()
}

fn net(e: reqwest::Error) -> Error {
    Error::Network(e.without_url().to_string())
}

impl Renderer {
    /// Fetches and reads the device description at `location`. Devices without
    /// an AVTransport service are not renderers: an error.
    pub async fn fetch(location: &str) -> Result<Renderer> {
        let http = client();
        let url = Url::parse(location).map_err(|e| Error::Description(e.to_string()))?;
        let xml = http
            .get(url.clone())
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(net)?
            .text()
            .await
            .map_err(net)?;
        Self::from_description(&xml, &url, http)
    }

    fn from_description(xml: &str, location: &Url, http: reqwest::Client) -> Result<Renderer> {
        let d = description::parse(xml, location)?;
        let av = d
            .service(AV_TRANSPORT)
            .ok_or_else(|| Error::Description(format!("{} has no AVTransport service", d.friendly_name)))?;
        Ok(Renderer {
            name: d.friendly_name.clone(),
            udn: d.udn.clone(),
            model: d.model_name.clone(),
            av_control: av.control_url.clone(),
            rc_control: d.service(RENDERING_CONTROL).map(|s| s.control_url.clone()),
            http,
        })
    }

    /// A renderer from known control URLs (tests, saved devices).
    pub fn new(name: &str, udn: &str, av_control: Url, rc_control: Option<Url>) -> Renderer {
        Renderer {
            name: name.to_string(),
            udn: udn.to_string(),
            model: String::new(),
            av_control,
            rc_control,
            http: client(),
        }
    }

    /// The renderer's address.
    pub fn ip(&self) -> Option<IpAddr> {
        match self.av_control.host()? {
            url::Host::Ipv4(a) => Some(IpAddr::V4(a)),
            url::Host::Ipv6(a) => Some(IpAddr::V6(a)),
            url::Host::Domain(d) => (d, 80).to_socket_addrs_first(),
        }
    }

    pub fn has_volume(&self) -> bool {
        self.rc_control.is_some()
    }

    async fn call(
        &self,
        control: &Url,
        service: &str,
        action: &str,
        args: &[(&str, &str)],
    ) -> Result<HashMap<String, String>> {
        let body = soap::envelope(service, action, args);
        let resp = self
            .http
            .post(control.clone())
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .header("SOAPACTION", soap::action_header(service, action))
            .body(body)
            .send()
            .await
            .map_err(net)?;
        let status = resp.status();
        let text = resp.text().await.map_err(net)?;
        // UPnP faults come with HTTP 500 and still carry the details.
        match soap::parse_response(&text) {
            Ok(out) if status.is_success() => Ok(out),
            Ok(_) => Err(Error::Network(format!("HTTP {status}"))),
            Err(e @ soap::SoapError::Fault { .. }) => Err(e.into()),
            Err(_) if !status.is_success() => Err(Error::Network(format!("HTTP {status}"))),
            Err(e) => Err(e.into()),
        }
    }

    async fn av(&self, action: &str, args: &[(&str, &str)]) -> Result<HashMap<String, String>> {
        self.call(&self.av_control, AV_TRANSPORT, action, args).await
    }

    async fn rc(&self, action: &str, args: &[(&str, &str)]) -> Result<HashMap<String, String>> {
        let control =
            self.rc_control.as_ref().ok_or_else(|| Error::Description("no RenderingControl".into()))?;
        self.call(control, RENDERING_CONTROL, action, args).await
    }

    /// Loads `uri` (the renderer fetches it itself) with DIDL-Lite metadata.
    pub async fn set_uri(&self, uri: &str, mime: &str, meta: &TrackMeta) -> Result<()> {
        let didl = soap::didl_lite(meta, uri, mime);
        self.av(
            "SetAVTransportURI",
            &[("InstanceID", "0"), ("CurrentURI", uri), ("CurrentURIMetaData", &didl)],
        )
        .await
        .map(drop)
    }

    pub async fn play(&self) -> Result<()> {
        self.av("Play", &[("InstanceID", "0"), ("Speed", "1")]).await.map(drop)
    }

    pub async fn pause(&self) -> Result<()> {
        self.av("Pause", &[("InstanceID", "0")]).await.map(drop)
    }

    pub async fn stop(&self) -> Result<()> {
        self.av("Stop", &[("InstanceID", "0")]).await.map(drop)
    }

    pub async fn seek(&self, to: Duration) -> Result<()> {
        let target = soap::format_time(to);
        self.av("Seek", &[("InstanceID", "0"), ("Unit", "REL_TIME"), ("Target", &target)]).await.map(drop)
    }

    pub async fn position_info(&self) -> Result<PositionInfo> {
        let r = self.av("GetPositionInfo", &[("InstanceID", "0")]).await?;
        let time = |k: &str| r.get(k).and_then(|t| soap::parse_time(t));
        Ok(PositionInfo {
            position: time("RelTime"),
            duration: time("TrackDuration").filter(|d| !d.is_zero()),
            uri: r.get("TrackURI").cloned().unwrap_or_default(),
        })
    }

    pub async fn transport_state(&self) -> Result<TransportState> {
        let r = self.av("GetTransportInfo", &[("InstanceID", "0")]).await?;
        Ok(r.get("CurrentTransportState").map_or(TransportState::Other, |s| TransportState::parse(s)))
    }

    /// 0..=100.
    pub async fn volume(&self) -> Result<u8> {
        let r = self.rc("GetVolume", &[("InstanceID", "0"), ("Channel", "Master")]).await?;
        Ok(r.get("CurrentVolume").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(100) as u8)
    }

    pub async fn set_volume(&self, volume: u8) -> Result<()> {
        let v = volume.min(100).to_string();
        self.rc("SetVolume", &[("InstanceID", "0"), ("Channel", "Master"), ("DesiredVolume", &v)])
            .await
            .map(drop)
    }

    pub async fn muted(&self) -> Result<bool> {
        let r = self.rc("GetMute", &[("InstanceID", "0"), ("Channel", "Master")]).await?;
        Ok(r.get("CurrentMute").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    }

    pub async fn set_mute(&self, mute: bool) -> Result<()> {
        let v = if mute { "1" } else { "0" };
        self.rc("SetMute", &[("InstanceID", "0"), ("Channel", "Master"), ("DesiredMute", v)]).await.map(drop)
    }
}

trait FirstAddr {
    fn to_socket_addrs_first(self) -> Option<IpAddr>;
}

impl FirstAddr for (&str, u16) {
    fn to_socket_addrs_first(self) -> Option<IpAddr> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().ok()?.next().map(|a| a.ip())
    }
}

/// This host's address on the network that reaches `remote` (what the renderer
/// has to call to fetch from it). Nothing is sent.
pub fn local_ip_for(remote: IpAddr) -> Option<IpAddr> {
    let bind: SocketAddr = if remote.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse().ok()?;
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect((remote, 9)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// Searches the LAN for media renderers for `wait` and reads their
/// descriptions. Sorted by name; one entry per device.
pub async fn discover(wait: Duration) -> Vec<Renderer> {
    discover_at(ssdp::MULTICAST, wait).await
}

/// [`discover`] against another SSDP address (tests).
pub async fn discover_at(target: SocketAddr, wait: Duration) -> Vec<Renderer> {
    let hits = match ssdp::search(target, ssdp::MEDIA_RENDERER, wait).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("DLNA search failed: {e}");
            return Vec::new();
        }
    };
    let mut tasks = tokio::task::JoinSet::new();
    for hit in hits {
        tasks.spawn(async move { Renderer::fetch(&hit.location).await });
    }
    let mut found: Vec<Renderer> = Vec::new();
    while let Some(done) = tasks.join_next().await {
        match done {
            Ok(Ok(r)) if !found.iter().any(|f| f.udn == r.udn) => found.push(r),
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::debug!("not a usable renderer: {e}"),
            Err(_) => {}
        }
    }
    found.sort_by_key(|r| r.name.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_states_parse() {
        assert_eq!(TransportState::parse("PLAYING"), TransportState::Playing);
        assert_eq!(TransportState::parse("PAUSED_PLAYBACK"), TransportState::Paused);
        assert_eq!(TransportState::parse("stopped"), TransportState::Stopped);
        assert_eq!(TransportState::parse("NO_MEDIA_PRESENT"), TransportState::NoMedia);
        assert_eq!(TransportState::parse("???"), TransportState::Other);
    }

    #[test]
    fn local_ip_for_loopback_is_loopback() {
        assert_eq!(local_ip_for("127.0.0.1".parse().unwrap()), Some("127.0.0.1".parse().unwrap()));
    }
}

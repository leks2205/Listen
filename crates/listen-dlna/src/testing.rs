//! A fake loopback DLNA renderer for tests: SSDP responder, device description and SOAP endpoints, with
//! state a test can read and set.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

#[derive(Default, Debug)]
pub struct State {
    pub uri: String,
    pub didl: String,
    /// The AVTransport state: `PLAYING`, `PAUSED_PLAYBACK`, `STOPPED`, ...
    pub state: &'static str,
    pub position: u64,
    pub volume: u32,
    pub muted: bool,
    /// Every action received, in order, with its arguments.
    pub log: Vec<(String, HashMap<String, String>)>,
}

fn time(s: u64) -> String {
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn parse_time(t: &str) -> u64 {
    t.split(':').fold(0, |acc, p| acc * 60 + p.parse::<u64>().unwrap())
}

async fn handle(mut s: TcpStream, dev: Arc<Mutex<State>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head_end, len) = loop {
        let n = s.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                .unwrap_or(0);
            break (i + 4, len);
        }
    };
    while buf.len() < head_end + len {
        let n = s.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let (status, reply) = if path == "/desc.xml" {
        let xml = String::from(
            r#"<?xml version="1.0"?><root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Fake Speaker</friendlyName>
<modelName>Fake 1</modelName><UDN>uuid:fake-1</UDN><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/avt</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
</serviceList></device></root>"#,
        );
        ("200 OK", xml)
    } else {
        let action = head
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .starts_with("soapaction:")
                    .then(|| l.split_once(':').unwrap().1.trim().to_string())
            })
            .and_then(|a| a.trim_matches('"').rsplit_once('#').map(|(_, a)| a.to_string()))
            .unwrap_or_default();
        let doc = roxmltree::Document::parse(&body).unwrap();
        let call = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == action).unwrap();
        let args: HashMap<String, String> = call
            .children()
            .filter(|n| n.is_element())
            .map(|n| (n.tag_name().name().to_string(), n.text().unwrap_or("").to_string()))
            .collect();
        let mut d = dev.lock().unwrap();
        d.log.push((action.clone(), args.clone()));
        let ns = if path == "/avt" { "AVTransport" } else { "RenderingControl" };
        let out: Result<String, (u32, &str)> = match action.as_str() {
            "SetAVTransportURI" => {
                d.uri = args["CurrentURI"].clone();
                d.didl = args["CurrentURIMetaData"].clone();
                d.state = "STOPPED";
                d.position = 0;
                Ok(String::new())
            }
            "Play" if d.uri.is_empty() => Err((701, "Transition not available")),
            "Play" => {
                d.state = "PLAYING";
                Ok(String::new())
            }
            "Pause" => {
                d.state = "PAUSED_PLAYBACK";
                Ok(String::new())
            }
            "Stop" => {
                d.state = "STOPPED";
                d.position = 0;
                Ok(String::new())
            }
            "Seek" => {
                d.position = parse_time(&args["Target"]);
                Ok(String::new())
            }
            "GetTransportInfo" => Ok(format!(
                "<CurrentTransportState>{}</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus><CurrentSpeed>1</CurrentSpeed>",
                d.state
            )),
            "GetPositionInfo" => Ok(format!(
                "<Track>1</Track><TrackDuration>0:03:25</TrackDuration><TrackMetaData></TrackMetaData><TrackURI>{}</TrackURI><RelTime>{}</RelTime><AbsTime>NOT_IMPLEMENTED</AbsTime>",
                d.uri.replace('&', "&amp;"),
                time(d.position)
            )),
            "SetVolume" => {
                d.volume = args["DesiredVolume"].parse().unwrap();
                Ok(String::new())
            }
            "GetVolume" => Ok(format!("<CurrentVolume>{}</CurrentVolume>", d.volume)),
            "SetMute" => {
                d.muted = args["DesiredMute"] == "1";
                Ok(String::new())
            }
            "GetMute" => Ok(format!("<CurrentMute>{}</CurrentMute>", u8::from(d.muted))),
            _ => Err((401, "Invalid Action")),
        };
        match out {
            Ok(inner) => (
                "200 OK",
                format!(
                    "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:{ns}:1\">{inner}</u:{action}Response></s:Body></s:Envelope>"
                ),
            ),
            Err((code, text)) => (
                "500 Internal Server Error",
                format!(
                    "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>{code}</errorCode><errorDescription>{text}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"
                ),
            ),
        }
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let _ = s.write_all(resp.as_bytes()).await;
}

/// A running fake renderer. Its tasks end with the test's runtime.
pub struct FakeRenderer {
    device: Arc<Mutex<State>>,
    /// Where its SSDP responder listens (for `renderer::discover_at`).
    pub ssdp: SocketAddr,
    /// The device description URL.
    pub location: String,
}

impl FakeRenderer {
    pub async fn start() -> FakeRenderer {
        let device: Arc<Mutex<State>> =
            Arc::new(Mutex::new(State { state: "NO_MEDIA_PRESENT", volume: 30, ..Default::default() }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let dev = device.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(handle(s, dev.clone()));
            }
        });
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let ssdp = udp.local_addr().unwrap();
        let answer = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {base}/desc.xml\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:fake-1::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n"
        );
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok((n, from)) = udp.recv_from(&mut buf).await {
                if buf[..n].starts_with(b"M-SEARCH") {
                    let _ = udp.send_to(answer.as_bytes(), from).await;
                }
            }
        });
        FakeRenderer { device, ssdp, location: format!("{base}/desc.xml") }
    }

    /// The renderer's state (hold the guard briefly).
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.device.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The names of the actions received so far.
    pub fn actions(&self) -> Vec<String> {
        self.state().log.iter().map(|(a, _)| a.clone()).collect()
    }
}

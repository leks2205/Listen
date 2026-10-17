//! The UPnP device description (`LOCATION` of an SSDP answer).

use crate::{Error, Result};
use url::Url;

pub const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
pub const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

/// One service of a device, with its control URL made absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub service_type: String,
    pub control_url: Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescription {
    pub friendly_name: String,
    pub udn: String,
    pub device_type: String,
    pub manufacturer: String,
    pub model_name: String,
    pub services: Vec<Service>,
}

impl DeviceDescription {
    /// The service of this family (`...:AVTransport:`), whatever its version.
    pub fn service(&self, family: &str) -> Option<&Service> {
        let family = family.rsplit_once(':').map_or(family, |(head, _)| head);
        self.services.iter().find(|s| s.service_type.rsplit_once(':').is_some_and(|(head, _)| head == family))
    }
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> String {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
        .and_then(|c| c.text())
        .map(|t| t.trim().to_string())
        .unwrap_or_default()
}

/// Parses a device description from `location`; control URLs resolve against `<URLBase>` or it.
pub fn parse(xml: &str, location: &Url) -> Result<DeviceDescription> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| Error::Description(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "root" {
        return Err(Error::Description("no <root> element".into()));
    }
    let base = root
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "URLBase")
        .and_then(|c| c.text())
        .and_then(|t| Url::parse(t.trim()).ok())
        .unwrap_or_else(|| location.clone());
    let device = root
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "device")
        .ok_or_else(|| Error::Description("no <device> element".into()))?;
    let mut services = Vec::new();
    // Services of embedded devices count too (some renderers nest them).
    for service in device.descendants().filter(|n| n.is_element() && n.tag_name().name() == "service") {
        let service_type = child_text(service, "serviceType");
        let control = child_text(service, "controlURL");
        if service_type.is_empty() || control.is_empty() {
            continue;
        }
        if let Ok(control_url) = base.join(&control) {
            services.push(Service { service_type, control_url });
        }
    }
    let friendly_name = child_text(device, "friendlyName");
    Ok(DeviceDescription {
        friendly_name: if friendly_name.is_empty() { child_text(device, "modelName") } else { friendly_name },
        udn: child_text(device, "UDN"),
        device_type: child_text(device, "deviceType"),
        manufacturer: child_text(device, "manufacturer"),
        model_name: child_text(device, "modelName"),
        services,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const FIXTURE: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Living Room</friendlyName>
    <manufacturer>Acme</manufacturer>
    <modelName>Streamer 9</modelName>
    <UDN>uuid:abcd-1234</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
        <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
        <SCPDURL>/AVT/scpd.xml</SCPDURL>
        <controlURL>/AVT/control</controlURL>
        <eventSubURL>/AVT/event</eventSubURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
        <controlURL>RC/control</controlURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
        <controlURL>/CM/control</controlURL>
      </service>
    </serviceList>
  </device>
</root>"#;

    #[test]
    fn parses_name_udn_and_services() {
        let loc = Url::parse("http://192.168.1.40:49152/desc/device.xml").unwrap();
        let d = parse(FIXTURE, &loc).unwrap();
        assert_eq!(d.friendly_name, "Living Room");
        assert_eq!(d.udn, "uuid:abcd-1234");
        assert_eq!(d.manufacturer, "Acme");
        assert_eq!(d.services.len(), 3);
        assert_eq!(
            d.service(AV_TRANSPORT).unwrap().control_url.as_str(),
            "http://192.168.1.40:49152/AVT/control"
        );
        // Relative control URLs are resolved against the description's folder.
        assert_eq!(
            d.service(RENDERING_CONTROL).unwrap().control_url.as_str(),
            "http://192.168.1.40:49152/desc/RC/control"
        );
    }

    #[test]
    fn urlbase_wins_and_services_match_any_version() {
        let xml = FIXTURE
            .replace("<device>", "<URLBase>http://10.0.0.5:8080/</URLBase><device>")
            .replace("AVTransport:1</serviceType>", "AVTransport:2</serviceType>");
        let loc = Url::parse("http://192.168.1.40/x.xml").unwrap();
        let d = parse(&xml, &loc).unwrap();
        assert_eq!(d.service(AV_TRANSPORT).unwrap().control_url.as_str(), "http://10.0.0.5:8080/AVT/control");
    }

    #[test]
    fn broken_documents_are_errors() {
        let loc = Url::parse("http://x/").unwrap();
        assert!(parse("not xml", &loc).is_err());
        assert!(parse("<root></root>", &loc).is_err());
        assert!(parse("<other/>", &loc).is_err());
    }
}

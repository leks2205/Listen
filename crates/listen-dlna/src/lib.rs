//! DLNA / UPnP casting: find media renderers on the LAN (SSDP), drive them (AVTransport + RenderingControl
//! SOAP actions) and serve local or header-protected audio to them through a small HTTP proxy.
//! No UPnP library; the protocol surface needed is small.

pub mod description;
pub mod proxy;
pub mod renderer;
pub mod soap;
pub mod ssdp;
#[doc(hidden)]
pub mod testing;

pub use description::{DeviceDescription, Service};
pub use proxy::{Proxy, Source};
pub use renderer::{PositionInfo, Renderer, TransportState, discover, local_ip_for};
pub use soap::SoapError;

/// Everything that can go wrong talking to a renderer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(String),
    #[error("the device description is not usable: {0}")]
    Description(String),
    #[error(transparent)]
    Soap(#[from] SoapError),
}

pub type Result<T> = std::result::Result<T, Error>;

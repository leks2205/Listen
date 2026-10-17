//! CD ripping for Listen (Linux). whipper does the secure rip (cd-paranoia, Test & Copy, AccurateRip,
//! offset, pregaps) as a subprocess; Listen identifies the disc on MusicBrainz, tags the files (FLAC
//! level 8, cover, ReplayGain), names them like MusicBee rips and files the album into the folder
//! Navidrome scans, never deleting anything there.

pub mod cover;
pub mod detect;
pub mod drive;
pub mod flac;
pub mod history;
pub mod layout;
pub mod musicbrainz;
pub mod notify;
pub mod pipeline;
pub mod publish;
pub mod queue;
pub mod ripper;
pub mod tags;
pub mod toc;
pub mod whipper;

pub use drive::DriveInfo;
pub use pipeline::{Command, JobConfig, Lookup, Stage, Update};
pub use ripper::Ripper;
pub use toc::Toc;

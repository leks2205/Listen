//! One rip at a time: a disc inserted while another is being ripped (in a
//! second drive) waits; tagging and publishing run behind the next rip.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct RipQueue {
    ripping: Option<PathBuf>,
    waiting: VecDeque<PathBuf>,
}

impl RipQueue {
    /// A disc is ready in `device`: `true` = start ripping it now.
    pub fn offer(&mut self, device: &Path) -> bool {
        if self.ripping.as_deref() == Some(device) || self.waiting.iter().any(|d| d == device) {
            return false;
        }
        if self.ripping.is_none() {
            self.ripping = Some(device.to_path_buf());
            true
        } else {
            self.waiting.push_back(device.to_path_buf());
            false
        }
    }

    /// The current rip is over (ripped, failed or cancelled): the next
    /// waiting drive, which is now the one ripping.
    pub fn drive_free(&mut self, device: &Path) -> Option<PathBuf> {
        if self.ripping.as_deref() != Some(device) {
            return None;
        }
        self.ripping = self.waiting.pop_front();
        self.ripping.clone()
    }

    /// The disc left a waiting drive.
    pub fn removed(&mut self, device: &Path) {
        self.waiting.retain(|d| d != device);
    }

    pub fn ripping(&self) -> Option<&Path> {
        self.ripping.as_deref()
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_rip_at_a_time() {
        let mut q = RipQueue::default();
        let (a, b, c) = (Path::new("/dev/sr0"), Path::new("/dev/sr1"), Path::new("/dev/sr2"));
        assert!(q.offer(a));
        assert!(!q.offer(a), "same drive twice");
        assert!(!q.offer(b));
        assert!(!q.offer(c));
        assert_eq!(q.waiting(), 2);
        q.removed(c);
        assert_eq!(q.drive_free(b), None, "not the one ripping");
        assert_eq!(q.drive_free(a).as_deref(), Some(b));
        assert_eq!(q.ripping(), Some(b));
        assert_eq!(q.drive_free(b), None);
        assert!(q.offer(a));
    }
}

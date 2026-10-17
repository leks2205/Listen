//! First-start wizard: language, features, analysis folder, the AI DJ's Gemini key and (Windows)
//! the helper programs, then the sign-in page. Shown once while nothing is set up; steps can be skipped.

use super::{Listen, Message};
use iced::Task;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Welcome,
    Features,
    Analysis,
    AiDj,
    Helpers,
}

impl Step {
    /// The steps on this platform, in order.
    pub fn all() -> Vec<Step> {
        let mut v = vec![Self::Welcome, Self::Features, Self::Analysis, Self::AiDj];
        if cfg!(windows) {
            v.push(Self::Helpers);
        }
        v
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Features => "Features",
            Self::Analysis => "Analysis folder",
            Self::AiDj => "AI DJ",
            Self::Helpers => "Helper programs",
        }
    }
}

#[derive(Debug, Clone)]
pub enum OnboardMsg {
    Go(Step),
    Next,
    Back,
    /// Language code; applies at once (`""` = the system's).
    Language(String),
    /// Ends the wizard; the sign-in page comes next.
    Finish,
    OpenUrl(&'static str),
}

#[derive(Debug, Default)]
pub struct OnboardState {
    pub step: Step,
}

/// Where a Gemini key is made.
pub const AI_STUDIO_KEYS: &str = "https://aistudio.google.com/apikey";

impl Listen {
    /// The wizard shows until it is finished, and only while no server is set up.
    pub fn onboarding_pending(&self) -> bool {
        if self.force_onboarding {
            return !self.config.settings.onboarded;
        }
        !self.demo && !self.ephemeral && !self.config.settings.onboarded && self.config.servers.is_empty()
    }

    pub(super) fn on_onboard(&mut self, msg: OnboardMsg) -> Task<Message> {
        let steps = Step::all();
        let at = steps.iter().position(|s| *s == self.onboard.step).unwrap_or(0);
        match msg {
            OnboardMsg::Go(step) => self.onboard.step = step,
            OnboardMsg::Next => match steps.get(at + 1) {
                Some(next) => self.onboard.step = *next,
                None => return self.on_onboard(OnboardMsg::Finish),
            },
            OnboardMsg::Back => self.onboard.step = steps[at.saturating_sub(1)],
            OnboardMsg::Language(code) => {
                self.config.settings.appearance.language = code.clone();
                crate::i18n::set_language(crate::i18n::resolve(&code));
                self.save_config();
            }
            OnboardMsg::Finish => {
                self.config.settings.onboarded = true;
                self.save_config();
            }
            OnboardMsg::OpenUrl(url) => super::album_page::open_url(url),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_start_with_welcome_and_end_with_the_ai_dj_or_helpers() {
        let steps = Step::all();
        assert_eq!(steps[0], Step::Welcome);
        assert_eq!(*steps.last().unwrap(), if cfg!(windows) { Step::Helpers } else { Step::AiDj });
    }
}

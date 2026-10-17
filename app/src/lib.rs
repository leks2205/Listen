//! Listen, a Feishin-style music player. `main.rs` only parses arguments and starts iced;
//! everything else lives here so tests can render views headlessly.

pub mod app;
pub mod appearance;
pub mod check;
pub mod check_mix;
pub mod check_ytm;
pub mod demo;
pub mod desktop_theme;
pub mod dialog;
pub mod discord;
#[cfg(target_os = "linux")]
pub mod global_shortcuts;
pub mod hotkeys;
pub mod i18n;
pub mod icons;
pub mod images;
pub mod inhibit;
pub mod logging;
pub mod mcp;
pub mod media_types;
pub mod milkdrop;
#[cfg(target_os = "linux")]
pub mod mpris;
/// The System Media Transport Controls, under the name the app code uses.
#[cfg(windows)]
#[path = "smtc.rs"]
pub mod mpris;
pub mod notify;
pub mod playback;
pub mod remote;
pub mod secrets;
pub mod single_instance;
pub mod style;
#[cfg(target_os = "linux")]
pub mod tray;
#[cfg(windows)]
#[path = "tray_win.rs"]
pub mod tray;
pub mod ui;
pub mod visualizer;

use std::path::PathBuf;

pub const FONTS: [&[u8]; 8] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../assets/fonts/Poppins-Regular.ttf"),
    include_bytes!("../../assets/fonts/Poppins-Medium.ttf"),
    include_bytes!("../../assets/fonts/Poppins-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Poppins-Bold.ttf"),
];

pub const USAGE: &str = "\
Usage: listen [OPTIONS]

Options:
  --demo                 Use the built-in demo library (no server needed)
  --queue                Open the queue sidebar on start (with --demo)
  --video                Start with the video switch on (with --demo)
  --theme <ID>           Force a Feishin theme id (e.g. tokyoNight, defaultLight)
  --size <WxH>           Initial window size (default 1600x950)
  --screenshot <PATH>    Save a PNG of the window after start-up, then exit
  --screenshot-delay <MS>  Wait before the screenshot (default 2500)
  --login <URL> <USER>   Sign in for this run only (password from $LISTEN_PASSWORD;
                         nothing is saved)
  --route <PAGE>         Start on home, albums, tracks or settings
  --autoplay             Start playing the first home album (with --login)
  --ephemeral            Don't save settings or credentials (tests, demos)
  --onboarding           Show the first-start wizard (with --ephemeral: screenshots)
  --check-youtube-music  Sign in with the browser's YouTube session and print what
                         Listen sees of the library (counts only), then exit
  --check-ai-dj [--no-voice]  Run two AI DJ rounds against Gemini with a sample
                         song pool and save the spoken breaks as WAV, then exit
  --check-cd             CD ripping check without ripping: tools, drive and read
                         offset, the disc's TOC and MusicBrainz match, the music
                         folder and Navidrome, then exit
  -h, --help             Show this help
  -V, --version          Show the version";

#[derive(Debug, Clone)]
pub struct Args {
    pub demo: bool,
    /// Demo songs play generated audio through mpv (off for screenshots and tests).
    pub demo_audio: bool,
    pub show_queue: bool,
    /// Start with the song/video switch on (for headless demo tests).
    pub video: bool,
    pub theme: Option<String>,
    pub size: (f32, f32),
    pub screenshot: Option<PathBuf>,
    pub screenshot_delay_ms: u64,
    /// Ephemeral sign-in: (url, user); password from `LISTEN_PASSWORD`.
    pub login: Option<(String, String)>,
    pub route: Option<String>,
    pub autoplay: bool,
    /// Never write config or credentials.
    pub ephemeral: bool,
    /// Show the first-start wizard even though this run is a demo or ephemeral.
    pub onboarding: bool,
}

impl Args {
    pub fn parse(mut it: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = Args {
            demo: false,
            demo_audio: false,
            show_queue: false,
            video: false,
            theme: None,
            size: (1600.0, 950.0),
            screenshot: None,
            screenshot_delay_ms: 2500,
            login: None,
            route: None,
            autoplay: false,
            ephemeral: false,
            onboarding: false,
        };
        while let Some(arg) = it.next() {
            let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
            match arg.as_str() {
                "--demo" => {
                    args.demo = true;
                    args.demo_audio = true;
                }
                "--queue" => args.show_queue = true,
                "--video" => args.video = true,
                "--theme" => args.theme = Some(value("--theme")?),
                "--size" => {
                    let v = value("--size")?;
                    let (w, h) = v.split_once('x').ok_or("--size expects WxH")?;
                    args.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
                }
                "--screenshot" => args.screenshot = Some(value("--screenshot")?.into()),
                "--screenshot-delay" => {
                    args.screenshot_delay_ms =
                        value("--screenshot-delay")?.parse().map_err(|_| "bad delay")?
                }
                "--login" => {
                    let url = value("--login")?;
                    let user = it.next().ok_or("--login needs <URL> <USER>")?;
                    args.login = Some((url, user));
                }
                "--route" => args.route = Some(value("--route")?),
                "--autoplay" => args.autoplay = true,
                "--ephemeral" => args.ephemeral = true,
                "--onboarding" => args.onboarding = true,
                "-h" | "--help" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                "-V" | "--version" => {
                    println!("listen {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
            }
        }
        Ok(args)
    }
}

impl Default for Args {
    fn default() -> Self {
        Self::parse(std::iter::empty()).expect("no arguments parse")
    }
}

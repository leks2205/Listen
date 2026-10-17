//! Listen binary entry point.

// No console window on Windows (release builds). `attach_parent_console` below
// gives `--version`, `--check-*` and friends their output back when started from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use listen::app::Listen;
use listen::{Args, FONTS};

/// With the GUI subsystem a process has no console. When started from cmd or
/// PowerShell, attach to it so `println!` output (`--version`, `--check-…`) shows.
#[cfg(windows)]
fn attach_parent_console() {
    use std::os::windows::io::IntoRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, handle: isize) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    // SAFETY: plain Win32 calls with valid arguments; handles we open are leaked on purpose.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for (which, name) in [(STD_OUTPUT_HANDLE, "CONOUT$"), (STD_ERROR_HANDLE, "CONOUT$")] {
            let current = GetStdHandle(which);
            // Redirected to a file or pipe by the shell: keep that. Otherwise use the console.
            if (current == 0 || current == -1)
                && let Ok(f) = std::fs::OpenOptions::new().write(true).open(name)
            {
                SetStdHandle(which, f.into_raw_handle() as isize);
            }
        }
    }
}

fn main() -> iced::Result {
    #[cfg(windows)]
    attach_parent_console();
    if std::env::args().nth(1).as_deref() == Some("--analysis-worker") {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        // SAFETY: no other thread runs yet.
        unsafe { listen_analysis::limit_threads(listen_analysis::thread_count(0)) };
        std::process::exit(listen_analysis::run_worker());
    }
    if std::env::args().nth(1).as_deref() == Some("--check-youtube-music") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        std::process::exit(listen::check::youtube_music());
    }
    if std::env::args().nth(1).as_deref() == Some("--check-ytm-session") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        std::process::exit(listen::check_ytm::session(std::env::args().skip(2).any(|a| a == "--write")));
    }
    if std::env::args().nth(1).as_deref() == Some("--check-ai-dj") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        let voice = !std::env::args().any(|a| a == "--no-voice");
        std::process::exit(listen::check::ai_dj(voice));
    }
    if std::env::args().nth(1).as_deref() == Some("--check-analysis") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        std::process::exit(listen::check::analysis(std::env::args().skip(2).collect()));
    }
    if std::env::args().nth(1).as_deref() == Some("--analyze-library") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        let args: Vec<String> = std::env::args().collect();
        let jobs = args
            .iter()
            .position(|a| a == "--jobs")
            .and_then(|i| args.get(i + 1))
            .and_then(|n| n.parse().ok())
            .unwrap_or(3);
        std::process::exit(listen::check::analyze_library(jobs));
    }
    if std::env::args().nth(1).as_deref() == Some("--check-mix") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        std::process::exit(listen::check_mix::run(&std::env::args().skip(2).collect::<Vec<_>>()));
    }
    if std::env::args().nth(1).as_deref() == Some("--check-cd") {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
            )
            .init();
        std::process::exit(listen::check::cd());
    }
    // Parsed first: `--version` / `--help` exit here without touching the log files.
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let settings = if args.demo {
        listen_core::settings::Settings::default()
    } else {
        listen_core::config::Paths::discover()
            .ok()
            .and_then(|p| listen_core::config::Config::load(&p.config_file()).ok())
            .map(|c| c.settings)
            .unwrap_or_default()
    };
    // SAFETY: no other thread runs yet (the logger and iced start below).
    unsafe { listen_analysis::limit_threads(listen_analysis::thread_count(settings.analysis.threads)) };
    listen::logging::init();

    // A relaunch (Windows) lets the old process release its single-instance lock first.
    if let Some(ms) = std::env::var("LISTEN_STARTUP_DELAY_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(5000)));
    }
    // One Listen per user: a second launch brings the running one forward and exits.
    if !listen::single_instance::exempt(&args) {
        use listen::single_instance::{Instance, acquire, forward_for, install, runtime_dir};
        match acquire(&runtime_dir(), &forward_for(args.route.as_deref())) {
            Ok(Instance::Primary(lock)) => install(lock),
            Ok(Instance::Secondary) => {
                eprintln!("Listen is already running; brought it forward.");
                std::process::exit(0);
            }
            Err(e) => tracing::warn!("single-instance lock unavailable: {e}"),
        }
    }

    let mut app = iced::daemon(move || Listen::boot(&args), Listen::update, Listen::window_view)
        .title(Listen::window_title)
        .theme(Listen::window_theme)
        .subscription(Listen::subscription)
        .scale_factor(Listen::ui_scale)
        .default_font(listen::appearance::font_for(&settings.appearance))
        .antialiasing(true);
    for font in FONTS {
        app = app.font(font);
    }
    app.run()
}

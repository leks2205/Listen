//! Cost of a full view rebuild + layout per page with a big library and a 1500-song
//! queue panel open (fullscreen queue tab included).
//! Run: cargo test --release -p listen --test view_cost -- --ignored --nocapture

use iced_test::simulator::Simulator;
use listen::app::{FullscreenTab, Listen, Message, Route};
use listen::{Args, demo};
use std::time::Instant;

const QUEUE: usize = 1500;

fn big_state() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        theme: Some("tokyoNight".into()),
        show_queue: true,
        ..Args::default()
    });
    let song = |i: usize| {
        let mut x = demo::album_songs(i % demo::ALBUMS.len())[0].clone();
        x.id = format!("s{i}");
        x
    };
    s.library.songs = (0..1415).map(song).collect();
    let albums: Vec<_> = (0..537)
        .map(|i| {
            let mut a = demo::albums()[i % demo::ALBUMS.len()].clone();
            a.id = format!("a{i}");
            a
        })
        .collect();
    s.library.albums = albums;
    s.queue.set((0..QUEUE).map(song).collect(), 3);
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: listen::FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: listen::app::INTER,
        ..Default::default()
    }
}

/// (view only, view + layout) in ms per rebuild.
fn measure(s: &Listen) -> (f64, f64) {
    let size = iced::Size::new(1849.0, 1233.0);
    // Warm up (text shaping caches), then measure.
    for _ in 0..3 {
        let _ = Simulator::with_size(settings(), size, s.view());
    }
    let n = 20;
    let t = Instant::now();
    for _ in 0..n {
        let _ = Simulator::with_size(settings(), size, s.view());
    }
    let total = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(s.view());
    }
    let view_only = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t = Instant::now();
    for _ in 0..n {
        let empty: iced::Element<'_, Message> = iced::widget::column![].into();
        let _ = Simulator::with_size(settings(), size, empty);
    }
    let overhead = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    (view_only, total - overhead)
}

#[test]
#[ignore]
fn view_rebuild_cost_per_page() {
    for route in [Route::Home, Route::Albums, Route::Tracks, Route::AlbumArtists, Route::Settings] {
        let mut s = big_state();
        let _ = s.update(Message::Navigate(route.clone()));
        let (view, all) = measure(&s);
        println!("{route:?} + {QUEUE}-song queue: view {view:.2} ms, layout ≈ {:.2} ms", all - view);
    }
    let mut s = big_state();
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Queue));
    let (view, all) = measure(&s);
    println!("fullscreen {QUEUE}-song queue: view {view:.2} ms, layout ≈ {:.2} ms", all - view);
}

/// The queue panel builds only visible rows: 1500 songs cost about what 15 do.
#[test]
fn a_long_queue_costs_no_more_than_a_short_one() {
    let mut long = big_state();
    let _ = long.update(Message::Navigate(Route::Settings));
    let mut short = big_state();
    let _ = short.update(Message::Navigate(Route::Settings));
    let songs: Vec<_> = long.queue.iter().take(15).map(|i| i.song.clone()).collect();
    short.queue.set(songs, 3);
    let time = |s: &Listen| {
        for _ in 0..3 {
            std::hint::black_box(s.view());
        }
        let t = Instant::now();
        for _ in 0..10 {
            std::hint::black_box(s.view());
        }
        t.elapsed()
    };
    let (long, short) = (time(&long), time(&short));
    assert!(long < short * 3, "1500-song queue {long:?} vs 15 songs {short:?}");
}

/// Reopening the queue (panel or fullscreen) resets the stored scroll offset to 0.
#[test]
fn queue_scroll_resets_when_the_queue_widget_is_recreated() {
    use listen::app::list::ListId;
    let mut s = big_state();
    let stale = |s: &mut Listen| {
        s.lists.entry(&ListId::SideQueue).scroll =
            listen::app::ScrollState { offset: 20_000.0, height: 800.0, width: 400.0 }
    };
    // Close and reopen the panel.
    let _ = s.update(Message::ToggleQueue);
    stale(&mut s);
    let _ = s.update(Message::ToggleQueue);
    assert_eq!(s.lists.scroll(&ListId::SideQueue).offset, 0.0);
    // Full-screen player: its queue tab.
    let _ = s.update(Message::ToggleFullscreen);
    stale(&mut s);
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Queue));
    assert_eq!(s.lists.scroll(&ListId::SideQueue).offset, 0.0);
    let _ = s.update(Message::FullscreenTab(FullscreenTab::Lyrics));
    stale(&mut s);
    let _ = s.update(Message::FullscreenTab(FullscreenTab::Queue));
    assert_eq!(s.lists.scroll(&ListId::SideQueue).offset, 0.0);
}

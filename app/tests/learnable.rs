//! The Learnable songs mix page.

mod common;

use iced_test::simulator::Simulator;
use listen::Args;
use listen::app::{Listen, Message, Route};

/// QA bug 94: at 960 px the progress line ("Looking at the lyrics of your liked songs: 396 of
/// 962") in the mix's subtitle ran past the window's right edge. It now wraps.
#[test]
fn the_progress_line_wraps_at_960_px() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = s.update(Message::Navigate(Route::Learnable));
    let _ = s.update(Message::WindowResized(iced::Size::new(960.0, 935.0)));
    let song = s.queue.current().unwrap().song.clone();
    s.learn.songs = vec![song.clone(), song.clone(), song];
    s.learn.loading = true;
    s.learn.checked = 396;
    s.learn.to_check = 962;
    s.learn.error = None;
    let total: std::time::Duration = s.learn.songs.iter().map(|x| x.duration).sum();
    let meta = format!(
        "Looking at the lyrics of your liked songs: 396 of 962 • 0 % to 0 % known words • 3 songs • {}",
        listen_core::model::format_duration(total)
    );
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(960.0, 935.0), s.view());
    let line = ui.find(meta.as_str()).map(|l| l.bounds());
    let line = line.expect("the progress line");
    // Unwrapped, the line keeps one row and runs out of the window; wrapped, it takes several rows
    // inside the header column.
    assert!(line.height > 30.0, "the line wraps to more rows: {line:?}");
    assert!(line.x + line.width <= 960.0, "the line ends inside the window: {line:?}");
}

//! Desktop notification when an album is done (org.freedesktop.Notifications)
//! and the completion sound (played with PipeWire's `pw-play`).

use std::path::Path;

/// Sound files tried in order (Plasma's Ocean theme, then freedesktop's).
const SOUNDS: [&str; 3] = [
    "/usr/share/sounds/ocean/stereo/completion-success.oga",
    "/usr/share/sounds/freedesktop/stereo/complete.oga",
    "/usr/share/sounds/freedesktop/stereo/complete.ogg",
];

pub async fn notify(summary: &str, body: &str, warning: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from("io.github.leks2205.Listen"));
        hints.insert("urgency", Value::U8(if warning { 2 } else { 1 }));
        let actions: Vec<&str> = Vec::new();
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Listen", 0u32, "io.github.leks2205.Listen", summary, body, actions, hints, -1i32),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (summary, body, warning);
        Ok(())
    }
}

/// Plays the completion sound once (best effort).
pub async fn play_sound() {
    let Some(file) = SOUNDS.iter().map(Path::new).find(|p| p.exists()) else { return };
    let _ = tokio::process::Command::new("pw-play")
        .arg(file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

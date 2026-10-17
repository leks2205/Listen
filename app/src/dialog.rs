//! Native file / folder pickers. Linux uses the desktop's file chooser portal
//! (shown in front of Listen) and falls back to `kdialog` / `zenity`; Windows
//! uses the Windows Forms dialogs through PowerShell. `None` if cancelled or no
//! dialog is available (the path can still be typed or the file dropped).

use std::path::{Path, PathBuf};
use tokio::process::Command;

/// What the picker should offer.
pub struct Pick<'a> {
    pub title: &'a str,
    /// Folder the dialog opens in (if it exists).
    pub start: Option<PathBuf>,
    /// Shown filter name, e.g. "Cookie exports".
    pub filter_name: &'a str,
    /// File name patterns, e.g. `["*.json", "*.txt"]`. Unused for folders.
    pub patterns: &'a [&'a str],
}

pub async fn open_file(pick: Pick<'_>) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Portal::Answered(path) = portal(&pick, false).await {
        return path;
    }
    run(commands(&pick, false)).await
}

pub async fn open_folder(pick: Pick<'_>) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Portal::Answered(path) = portal(&pick, true).await {
        return path;
    }
    run(commands(&pick, true)).await
}

#[cfg(target_os = "linux")]
enum Portal {
    /// The user picked something or cancelled.
    Answered(Option<PathBuf>),
    /// No file chooser portal; try the dialog tools.
    Unavailable,
}

#[cfg(target_os = "linux")]
async fn portal(pick: &Pick<'_>, folder: bool) -> Portal {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
    let mut request = SelectedFiles::open_file().title(pick.title).modal(true).directory(folder);
    if !folder && !pick.patterns.is_empty() {
        let filter = pick.patterns.iter().fold(FileFilter::new(pick.filter_name), |f, p| f.glob(p));
        request = request.filter(filter);
    }
    if let Some(dir) = pick.start.as_deref().filter(|p| p.is_dir()) {
        request = match request.current_folder(dir) {
            Ok(r) => r,
            Err(_) => return Portal::Unavailable,
        };
    }
    let reply = match request.send().await {
        Ok(reply) => reply,
        Err(e) => {
            tracing::debug!("file chooser portal unavailable: {e}");
            return Portal::Unavailable;
        }
    };
    match reply.response() {
        Ok(files) => Portal::Answered(files.uris().first().and_then(|u| u.to_file_path().ok())),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Portal::Answered(None),
        Err(e) => {
            tracing::debug!("file chooser portal failed: {e}");
            Portal::Unavailable
        }
    }
}

async fn run(commands: Vec<Command>) -> Option<PathBuf> {
    for mut cmd in commands {
        listen_core::platform::no_window(cmd.as_std_mut());
        match cmd.output().await {
            Ok(out) => {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return (out.status.success() && !path.is_empty()).then(|| path.into());
            }
            Err(_) => continue, // tool not installed; try the next one
        }
    }
    None
}

#[cfg(not(windows))]
fn commands(pick: &Pick<'_>, folder: bool) -> Vec<Command> {
    let start = pick.start.clone().unwrap_or_default();
    let mut kd = Command::new("kdialog");
    let mut zen = Command::new("zenity");
    if folder {
        kd.args(["--title", pick.title, "--getexistingdirectory"]).arg(&start);
        zen.args(["--file-selection", "--directory"]).arg(format!("--title={}", pick.title));
    } else {
        kd.args(["--title", pick.title, "--getopenfilename"]).arg(&start).arg(format!(
            "{} ({})",
            pick.filter_name,
            pick.patterns.join(" ")
        ));
        zen.args(["--file-selection"])
            .arg(format!("--title={}", pick.title))
            .arg(format!("--file-filter={}", pick.patterns.join(" ")));
    }
    vec![kd, zen]
}

#[cfg(windows)]
fn commands(pick: &Pick<'_>, folder: bool) -> Vec<Command> {
    let mut ps = Command::new("powershell");
    ps.args(["-NoProfile", "-NonInteractive", "-Command"]).arg(powershell_script(pick, folder));
    vec![ps]
}

/// The PowerShell snippet for a Windows Forms dialog; prints the chosen path.
#[cfg_attr(not(windows), allow(dead_code))]
fn powershell_script(pick: &Pick<'_>, folder: bool) -> String {
    let q = |s: &str| s.replace('\'', "''");
    let start = pick.start.as_deref().filter(|p| Path::new(p).is_dir()).map(|p| q(&p.display().to_string()));
    if folder {
        let mut s = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{}'; ",
            q(pick.title)
        );
        if let Some(start) = start {
            s += &format!("$d.SelectedPath = '{start}'; ");
        }
        s + "if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"
    } else {
        let globs = pick.patterns.join(";");
        let mut s = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{}'; $d.Filter = '{} ({globs})|{globs}|All files|*.*'; ",
            q(pick.title),
            q(pick.filter_name)
        );
        if let Some(start) = start {
            s += &format!("$d.InitialDirectory = '{start}'; ");
        }
        s + "if ($d.ShowDialog() -eq 'OK') { $d.FileName }"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_dialog_script_quotes_and_filters() {
        let pick = Pick {
            title: "Alex's cookies",
            start: None,
            filter_name: "Cookie exports",
            patterns: &["*.json", "*.txt"],
        };
        let file = powershell_script(&pick, false);
        assert!(file.contains("OpenFileDialog"));
        assert!(file.contains("$d.Title = 'Alex''s cookies'"));
        assert!(file.contains("Cookie exports (*.json;*.txt)|*.json;*.txt|All files|*.*"));
        let folder = powershell_script(&pick, true);
        assert!(folder.contains("FolderBrowserDialog") && folder.ends_with("$d.SelectedPath }"));
    }
}

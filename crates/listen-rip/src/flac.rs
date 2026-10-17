//! `flac` / `metaflac` (the FLAC reference tools): re-encode at level 8
//! with an audio check, write Vorbis comments, embed the cover, ReplayGain.

use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Encoder preset: the strongest standard level (decodes as fast as -5).
pub const LEVEL: &str = "-8";

async fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn os(s: &str) -> &std::ffi::OsStr {
    std::ffi::OsStr::new(s)
}

/// The audio MD5 stored in STREAMINFO (same for every lossless encoding).
pub async fn md5(path: &Path) -> Result<String, String> {
    let s = run("metaflac", &[os("--show-md5sum"), path.as_os_str()]).await?;
    Ok(s.trim().to_string())
}

/// Re-encodes at [`LEVEL`] in place; the audio MD5 must stay the same
/// (and `--verify` decodes the new file while writing it).
pub async fn recompress(path: &Path) -> Result<(), String> {
    let before = md5(path).await?;
    if before.chars().all(|c| c == '0') {
        return Err(format!("{}: no audio checksum in the file", path.display()));
    }
    let tmp = path.with_extension("level8.flac");
    let result = async {
        run(
            "flac",
            &[
                os(LEVEL),
                os("--verify"),
                os("--silent"),
                os("--force"),
                os("-o"),
                tmp.as_os_str(),
                path.as_os_str(),
            ],
        )
        .await?;
        let after = md5(&tmp).await?;
        if after != before {
            return Err(format!("{}: audio changed while re-encoding ({before} → {after})", path.display()));
        }
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
    .await;
    if result.is_err() {
        // Only our own temporary file (local temp dir).
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Replaces all Vorbis comments with `tags` (UTF-8 as is).
pub async fn write_tags(path: &Path, tags: &[(String, String)]) -> Result<(), String> {
    let list: PathBuf = path.with_extension("tags.txt");
    let body: String = tags.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    std::fs::write(&list, body).map_err(|e| e.to_string())?;
    let mut arg = std::ffi::OsString::from("--import-tags-from=");
    arg.push(&list);
    let r =
        run("metaflac", &[os("--no-utf8-convert"), os("--remove-all-tags"), &arg, path.as_os_str()]).await;
    let _ = std::fs::remove_file(&list);
    r.map(|_| ())
}

/// Embeds `image` as the front cover (picture type 3), replacing others.
pub async fn embed_picture(path: &Path, image: &Path) -> Result<(), String> {
    run("metaflac", &[os("--remove"), os("--block-type=PICTURE"), path.as_os_str()]).await?;
    let mut arg = std::ffi::OsString::from("--import-picture-from=3||||");
    arg.push(image);
    run("metaflac", &[&arg, path.as_os_str()]).await.map(|_| ())
}

/// Track and album ReplayGain over all files of one album.
pub async fn add_replay_gain(files: &[PathBuf]) -> Result<(), String> {
    let mut args: Vec<&std::ffi::OsStr> = vec![os("--add-replay-gain")];
    args.extend(files.iter().map(|f| f.as_os_str()));
    run("metaflac", &args).await.map(|_| ())
}

pub async fn read_tags(path: &Path) -> Result<Vec<(String, String)>, String> {
    let s = run("metaflac", &[os("--no-utf8-convert"), os("--export-tags-to=-"), path.as_os_str()]).await?;
    Ok(s.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_uppercase(), v.to_string())).collect())
}

pub async fn has_front_cover(path: &Path) -> bool {
    run("metaflac", &[os("--list"), os("--block-type=PICTURE"), path.as_os_str()])
        .await
        .is_ok_and(|s| s.contains("type: 3 (Cover (front))"))
}

/// Samples per channel in the file.
pub async fn sample_count(path: &Path) -> Result<u64, String> {
    let s = run("metaflac", &[os("--show-total-samples"), path.as_os_str()]).await?;
    s.trim().parse().map_err(|e| format!("{e}"))
}

/// Version line of a tool, or `None` if it isn't installed.
pub async fn tool_version(program: &str) -> Option<String> {
    let out = Command::new(program).arg("--version").output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    text.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string())
}

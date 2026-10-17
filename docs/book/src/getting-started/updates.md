# Updates

This page explains how Listen finds and installs new versions, and how to control it. Updates come from the project's public GitHub releases; no account or token is needed.

## How it works

When Listen starts, it asks GitHub for the list of releases (if **Check on start** is on). If there is a newer version for your release channel, an **Update available** dialog opens. It shows the new version, the release notes and what installing will do. Nothing is installed without your confirmation.

When you press **Download and install**, Listen downloads the package for your system together with the release's `SHA256SUMS` file and checks the package against it. If the checksum does not match, nothing is installed. A release without a `SHA256SUMS` file cannot be installed by the updater.

What happens next depends on how you installed Listen:

| Installation | What the updater does |
|---|---|
| Fedora RPM (program under `/usr`) | Installs the matching RPM for your Fedora release with `pkexec dnf install`. You are asked for your password. |
| Tarball or other copy under your home folder, such as `~/.local/bin/listen` | Replaces that file in place. Listen first checks that the new program reports the expected version. |
| Windows installer | Runs the new `Listen-Setup-<version>.exe` silently in the background, closes Listen and starts the new version. Your queue and window position are saved first. |

After an RPM or tarball update the dialog says "Installed. Restart Listen to use the new version." Press **Restart now**, or **Later**.

If Listen cannot update itself, the dialog explains why instead. This happens for a development build, for a Windows copy that was not set up by the installer (download the new installer from the release page), and for a program installed somewhere else, such as a package from another distribution (update it with the tool you installed it with).

## The choices in the dialog

| Button | Effect |
|---|---|
| **Download and install** | Installs as described above. |
| **Remind later** | Hides the dialog for 24 hours. |
| **Skip this version** | Never opens the dialog automatically for this version. A newer version is offered again. |

Skipping and reminding only affect the automatic dialog at start. You can always open the update yourself in the settings.

## Settings

Everything is in **Settings → Advanced → Updates**.

| Setting | Description | Default |
|---|---|---|
| **Check for updates** | **Check now** looks immediately and shows the result. | |
| **Check on start** | Look for a new version when Listen starts. | On |
| **Release channel** | **Stable** offers full releases only. **Beta** also offers pre-releases. | Stable |
| **Update available** | Appears when a newer version was found; **View update** opens the dialog, even for a skipped version. | |
| **Helper programs** | Windows only: **Show** and **Update** for mpv, yt-dlp and Deno. See [Installation](installation.md#helper-programs). | |
| **Release source** | **Open releases** opens the GitHub releases page. | |

The first-start wizard has the same switch as **Check for updates**.

## Notes

- A manual **Check now** always opens the dialog when an update exists, even if you skipped it or asked to be reminded.
- Updates are not checked in demo mode (`listen --demo`) or with `--ephemeral`.
- Switching to **Beta** is safe to undo: switch back to **Stable** and you are offered full releases again, but Listen does not downgrade by itself.
- Update checks are one of the few things Listen contacts without you adding a service. See [Privacy and security](../more/privacy.md). If you do not want them, switch **Check on start** off.

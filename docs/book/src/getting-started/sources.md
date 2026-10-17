# Connecting your music

This page explains how to add a Navidrome or OpenSubsonic server, a Jellyfin server and local folders, and how to manage several servers. YouTube Music has its own page: [YouTube Music](youtube-music.md).

## Open the sign-in page

On a fresh install the sign-in page appears after the [first-start wizard](first-start.md). Later, open it with **Add server** in the sidebar's server list, in the app menu, or in **Settings → General → Servers**.

The page has four tabs: **Navidrome**, **Jellyfin**, **YouTube Music** and **Local**. Below the form it lists **Your servers**, so a broken entry can be edited or removed from here too. If you already have a server, a **Cancel** button takes you back.

Passwords are stored in your system keyring (Secret Service on Linux, Credential Manager on Windows). If there is no keyring, they go in a credentials file in Listen's config folder that only your user can read. You can choose with **Settings → General → Servers → Password storage**: **System keyring** or **File only**.

## Navidrome and OpenSubsonic

Any server that speaks the OpenSubsonic (Subsonic) API works: Navidrome, Gonic, Airsonic and similar.

1. Choose the **Navidrome** tab.
2. Enter the **Server URL**, for example `https://music.example.com`, your **Username** and **Password**.
3. Press **Connect**.

Two switches adjust how Listen talks to the server:

| Option | What it does |
|---|---|
| **Legacy authentication** | Sends the password (hex-encoded) instead of a token and salt. Only for old servers that need it. |
| **Ignore certificate errors** | Accepts an invalid or self-signed certificate from this server only. The server's identity is then not checked, so someone on your network could read your password. Use it only for a server you run yourself. |

## Jellyfin

1. Choose the **Jellyfin** tab.
2. Enter the **Server URL**, for example `https://jellyfin.example.com`, plus **Username** and **Password**, and press **Connect**.

Only the server's music libraries are used.

**Discovery.** When you open the sign-in page (on a fresh install, or with **Add server**), Listen looks for Jellyfin servers on your local network and lists them under **Found on your network**. Press **Use** to fill in the address. Press **Scan again** to search again. Servers you already added are not listed.

**Quick Connect.** Instead of typing a password, press **Sign in with Quick Connect**. Listen asks the server for a code. On a device where you are already signed in to Jellyfin, open Quick Connect and enter the code, or press **Open the approval page**. Listen waits for the approval and signs in. **Cancel Quick Connect** stops waiting.

Jellyfin has two extra switches:

| Option | What it does |
|---|---|
| **Prefer instant mix** | Builds radio from Jellyfin's instant mix instead of asking for similar songs first. |
| **Ignore certificate errors** | As above. |

## Local folders

1. Choose the **Local** tab.
2. Type a path such as `~/Music`, or press **Browse…** and pick the folder.
3. Press **Connect**.

Listen reads the tags and cover art of the files in the folder and its subfolders (MP3, FLAC, Ogg, Opus, M4A, WAV and more). Large folders take a moment to scan. You can add more than one folder; each becomes its own entry in the server list.

After you add files, press **Rescan** next to the folder in **Settings → General → Servers**.

## Several servers side by side

Every source you add appears in the sidebar's server list at the top, with its type underneath. Servers that are not connected yet show "not connected".

- **Switch:** click a server in the sidebar, choose it in the app menu, or press **Switch** in **Settings → General → Servers** (the current one shows **Browsing**). Library pages show the active server.
- **One queue:** the queue can hold songs from different servers. Switching the browsing server does not clear it.
- **Edit:** press **Edit** next to a server in **Settings → General → Servers** to open the **Edit server** dialog.
- **Remove:** click the cross next to a server (**Remove server**), then confirm with **Yes**. In Settings the button reads **Remove**. This only removes the entry and its saved password from Listen, not anything on the server.

### Editing a server

The **Edit server** dialog shows the **Name** and, depending on the source:

- Navidrome and Jellyfin: **Server URL**, **Username**, **Password** (leave it empty to keep the saved one) and the option switches above. Saving checks the connection first (the button reads **Checking…**).
- Navidrome only: **Server music folder (optional)** and **Local music folder**. If your server's music folder is mounted on this computer (for example `/music` on the server and `/mnt/nas/Music` here), fill in both so that **Show in file manager** works for those songs. Leave them empty to hide that action.
- Local: **Music folder**.
- YouTube Music: change the channel, switch browser or **Reconnect (read the cookies again)**. See [YouTube Music](youtube-music.md).

## When a server needs attention

If the server you are browsing cannot be used when Listen starts or when you switch to it, Listen shows a page with the reason instead of an empty library.

| Page title | Meaning | Buttons |
|---|---|---|
| **Sign in to &lt;name&gt; again** | Listen has no saved password for this server, or the server no longer accepts it. | **Enter the password** (opens the edit dialog), **Try again** |
| **Could not reach &lt;name&gt;** | The server did not answer; the reason is shown. | **Try again**, **Edit server** |

Both pages also offer **Add a server**, a list of **Your other servers** with **Use** and remove buttons, and a way to remove the failing server. If you have downloaded songs, **Play downloaded songs (offline)** opens them without a server. See [Downloads](../using/downloads.md).

## Next steps

- [The window](../using/window.md) for a tour of the interface.
- [Library pages](../using/library.md) for browsing what you just connected.

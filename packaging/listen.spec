# Binary RPM for Listen. Built by packaging/build-rpm.sh from the release binary
# (see README.md); the source is on GitHub.
%global debug_package %{nil}
%global __strip /bin/true

Name:           listen
Version:        %{listen_version}
Release:        1%{?dist}
Summary:        Feishin-style music player for Navidrome and YouTube Music
License:        AGPL-3.0-or-later
URL:            https://github.com/leks2205/Listen
Source0:        listen-%{version}-x86_64.tar.gz
ExclusiveArch:  x86_64

# Playback, YouTube streams, visualizer capture.
Requires:       mpv
Recommends:     yt-dlp
Requires:       pipewire-utils
Requires:       hicolor-icon-theme
# Loaded at runtime with dlopen (winit/wgpu), so rpm can't detect them.
Requires:       libglvnd-egl
Requires:       libwayland-client
Requires:       libwayland-egl
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libX11
Requires:       libX11-xcb
Requires:       libxcb
Requires:       libXcursor
Requires:       libXi
Requires:       libXrandr
# CD ripping: whipper (secure rip + AccurateRip; pulls in cd-paranoia), flac/metaflac
# (level-8 re-encode, tags, cover, ReplayGain), cdrdao (TOC, pregaps, CD-Text).
Requires:       whipper
Requires:       flac
Requires:       cdrdao
# Drive detection and eject without root; the NAS music folder over SMB.
Recommends:     udisks2
Recommends:     cifs-utils
# yt-dlp needs a JavaScript runtime for YouTube.
Recommends:     nodejs
Recommends:     mesa-dri-drivers
Recommends:     mesa-vulkan-drivers
Recommends:     vulkan-loader
# Korean titles and lyrics are drawn with a system Korean font (Noto Sans CJK KR).
Recommends:     default-fonts-cjk-sans

%description
Listen is a native desktop music player written in Rust, with an interface
modelled on Feishin. It plays from Navidrome / OpenSubsonic and Jellyfin
servers, local folders and YouTube Music, with gapless playback through mpv,
DJ transitions, an optional AI DJ, synced lyrics, a visualizer, MPRIS media keys
and Feishin's themes. Audio CDs can be ripped to FLAC (whipper, AccurateRip,
MusicBrainz tags).

%prep
%setup -q -n listen-%{version}-x86_64

%install
install -Dm0755 listen %{buildroot}%{_bindir}/listen
install -Dm0644 io.github.leks2205.Listen.desktop %{buildroot}%{_datadir}/applications/io.github.leks2205.Listen.desktop
install -Dm0644 io.github.leks2205.Listen.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.leks2205.Listen.svg

%files
%license LICENSE NOTICE.md THIRD_PARTY_LICENSES.html
%doc README.md
%{_bindir}/listen
%{_datadir}/applications/io.github.leks2205.Listen.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.leks2205.Listen.svg

%changelog
* Mon Sep 28 2026 leks2205 <281423329+leks2205@users.noreply.github.com> - %{listen_version}-1
- Build from the Listen repository.

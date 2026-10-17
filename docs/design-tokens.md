# Feishin desktop design tokens (reference for Listen's pixel-close UI)

Extracted from Feishin v1.17.0 (`/tmp/feishin`, commit of 2026-09-27; Mantine 9.3). Paths are relative to Feishin's `src/`.
Values are for **Default Dark** at default settings unless noted. The reference preset uses **Tokyo Night** in dark mode
with accent `rgb(53,116,252)`; the derived-colour formulas below apply to any theme.

CSS colour helpers (postcss-preset-mantine):
- `alpha(c,a)` → multiply alpha by `a`
- `lighten(c,x%)` → per channel `c*(1-x)+255*x`
- `darken(c,x%)` → per channel `c*(1-x)`

## 1. Global
- **Font:** Inter (variable, 100–1000), tabular numbers, antialiased. Root font size 16px.
- **Font sizes (px):** xs 11, sm 13, md 14, lg 16, xl 18, 2xl 20, 3xl 24, 4xl 28, 5xl 32.
- **Line heights (px):** xs 14, sm 16, md 18, lg 20, xl 24.
- **Headings:** weight 900. h1 36/44, h2 30/38, h3 24/32, h4 20/30.
- **Text:** default 14px / 18 lh / weight 400. Common weights are 500 (titles), 600 (labels, times), 800 (library title) and 900 (hero).
- **Spacing (px):** xs 4, sm 8, md 12, lg 16, xl 24, 2xl 32, 3xl 36, 4xl 40.
- **Radii (px):** xs/sm 3, md 5, lg 12, xl 16. Buttons, inputs and action icons use 3; images and cards use 5.
- **Shadows:**
  - sm `0 1px 3px rgba(0,0,0,.1)`
  - md `0 4px 6px rgba(0,0,0,.1)`
  - lg `0 10px 15px rgba(0,0,0,.1)`
  - xxl `0 25px 50px rgba(0,0,0,.25)`
- **Content max width:** 1800px, centred.
- **Scrollbar:**
  - 9–12px wide.
  - Handle rgba(160,160,160,.2), .6 on hover, .4 when active.
  - Transparent track, radius 0.
- **Overlays:** `overlay-header` is a linear gradient from transparent to `rgb(0 0 0/85%)`, plus 5% SVG noise.
- **Transitions:**
  - Buttons: bg .2s ease-in-out.
  - Player bar bg: .5s.
  - Card overlay: .2s.
  - Lyric lines: .6s cubic-bezier(0.16,1,0.3,1).
- **Mantine sizes:**
  - Button heights: xs 30, sm 36, md 42, lg 50, xl 60. Padding-x 14/18/22/26/32. Font 13px weight 500.
  - ActionIcon: xs 30, sm 36, md 42, lg 50, xl 60. Compact: xs 24, sm 28, md 32.
  - Input height (sm): 36.

## 2. Layout
- **Root grid (windows-style bar):** window bar 30px, content `100vh-120px`, player bar **90px**.
- **Window bar:**
  - Height 30px, background `background`, border-bottom rgba(255,255,255,.05). Draggable.
  - Left side (padding-left 16px, gap 8px): 16px app icon, then 13px text `"{Paused }({i}/{n}) {song} — {artist} — Feishin"`.
  - Right side: 3 buttons of 50×30. Icons RiSubtractLine 19, RiCheckboxBlankLine 13, RiCloseLine 19.
  - Button hover rgb(125 125 125/30%). Close uses `state-error` on hover.
- **Main grid:** `sidebar | 1fr | [right sidebar]`.
  - **Left sidebar:** default **400px**, clamped 260–400. Dragging below 160 collapses it to **80px**.
  - **Right sidebar (queue):** default 600px, clamped 250–960, hidden by default. Background `background-alternate`, border-left rgba(255,255,255,.05).
- **Left sidebar:** background `background-alternate`, border-right rgba(255,255,255,.05). Flex column, gap 4.
  - **Action bar:** 65px high, padding 0 12. Grid gap 8.
    - Search input (7/12 of the width, 36px high, LuSearch icon; opens the command palette).
    - Three 36px buttons (LuMenu, LuChevronLeft, LuChevronRight, 16px icons).
  - **Scroll area:** padding 0 12 12 12.
  - **Accordion** ("My Library", "Playlists"):
    - Header min-height 40, radius 5, muted 14px weight 500, hover bg `background`.
    - The last panel has 12px bottom padding.
  - **Nav item:**
    - Subtle button, **36px** high, radius 3, padding-x 12, left-aligned.
    - Icon, then a 12px gap, then the label (14px, weight 500, ellipsis). The icon is about 14px (1em).
    - Colours: foreground; active uses `primary-filled`; hover bg `lighten(bg,10%)`.
  - **Playlist rows:**
    - List padding 8 12.
    - Row radius 5, hover bg `surface`, padding 4 12, gap 12.
    - Thumbnail 48px.
  - **Sidebar image:** square, full width, padding 12, radius 5. Hidden until expanded from the player-bar art.
- **Collapsed sidebar:** 80px wide.
  - Items padding .9rem .3rem. 25px Remix icon above an 11px weight-600 label.
  - Active item: a 2px indicator bar.
- **Page padding:**
  - Home: px 32, pt 48, mb 80.
  - Album detail: 16 32 80.
  - List grids: padding 0 12.

## 3. Player bar
- 90px high, background `darken(bg,10%)`, border-top rgba(255,255,255,.05).
- Three equal columns, gap 16px.
- **Left column:**
  - Padding-left 16. Art **60×60** (no radius), padding 12 12 12 0. Art zooms to 1.1 on hover.
  - Metadata is a column with gap 4, each line max 20vw with ellipsis:
    - title 14px weight 500, followed by an 18px "more" icon;
    - artists 14px muted;
    - album 14px weight 500, muted.
- **Centre column:**
  - Controls row 35px high, gap 8.
  - Button order: Stop, Shuffle, Prev, [SkipBack], **Play**, [SkipFwd], Next, Repeat, ShuffleAll.
  - Buttons are subtle ActionIcons **36×36**, radius 3, hover bg `lighten(bg,5%)`.
  - **Icon size = buttonSize setting = 15px** (Stop 13). Active shuffle/repeat uses `primary-filled`.
  - **Play button:** 36×36 circle, bg `foreground`, icon colour `background`, 16px LuPlay/LuPause.
- **Slider row:**
  - Width 95%, height 20.
  - Grid 76px | 1fr | 76px, gap 8.
  - Times: 11px, weight 600, muted.
- **Seek slider:**
  - Track 6px, pill-shaped. Bar colour `foreground`; `primary-filled` on hover.
  - Thumb 16px with a 1px primary border, visible only on hover.
- **Right column:**
  - Column aligned to the end, padding 16/8. Three rows, with the middle row holding the controls.
  - Middle row, gap 4: cast, sleep timer, player config, lyrics (LuMicVocal 16), favorite (LuHeart 16), queue (LuPanelRightOpen/Close 16), then volume.
  - Volume: LuVolume2/1/X at 18px, slider **70px** wide (volumeWidth), 6px track, wheel step 5.

## 4. Lists
- **Table:**
  - Row heights: **compact 40, default 66, large 88**. Header 40.
  - Cell padding 4 8. Header 13px, padding 0 8.
  - Text clamp: 2 lines at default, 1 at compact.
  - Title/artist cell: title 14px; artists 11px muted; gap 4. Image column 70px wide, radius 5.
  - Row hover bg `surface`. Selected `lighten(surface,5%)`. Borders are off by default.
  - Songs, playlists and the queue use the default size. Album detail tracks and genres use compact.
  - Album detail columns: # 50, Title 400, Duration 100, Favorite 60.
- **Grid cards:**
  - Gap `sm` = 8px padding per cell (16px between images). Container padding 0 12.
  - **Columns by container width:** <600: 2, ≥600: 3, ≥768: 4, ≥960: 5, ≥1200: 6, ≥1440: 7, ≥1920: 8, ≥2560: 10.
  - Cell height = width/cols + rows×26px.
  - Poster card:
    - Square image, radius 5. Hover shows a black overlay (opacity .7 dark, .5 light).
    - Text starts 8px below the image, gap 4. Row 0 is 14px weight 500; the other rows are 13px muted.
    - Rows are Title / Album Artist / Year.
  - Selected card: 2px primary outline at offset 4.
  - Hover controls: centre play button 25% of the image width (white circle, black icon); side buttons 15%.
- **"Detail" album display (reference preset's albums view):**
  - Each row is a grid 240px | 1fr, gap 12, padding 12, border-bottom `border`.
  - Left column: 240px image, radius 5, with centred metadata below it (14px title weight 500, 13px muted artist).
  - Right column: a 13px track table. Row height compact 32, default 40, large 48; padding 0 8.

## 5. Headers, buttons, menus
- **Library header:**
  - Below 768px: stacked, 250px image, padding 32 16.
  - ≥768px: grid 225px | 1fr, gap 12, bottom-aligned, min-height 340, padding 80 32 32, 225px image.
  - ≥1200px: 250px image.
  - Image radius 5, drop-shadow 0 0 8px rgba(0,0,0,.5).
  - Metadata gap 8:
    - type label 14px, weight 600, uppercase;
    - title weight 800, lh 1.2, size clamp(28px, 3dvw, 44px). The dvw factor shrinks with title length.
- **Header background:**
  - Artist pages (and albums when enabled): blurred image (blur = setting in rem; reference preset: 3) at opacity .7, with the `overlay-subheader` gradient at .9 on top.
  - Otherwise: the image's average colour at .7.
- **Header buttons:**
  - Play / Add next / Add last: 36px high, padding 0 24, **radius 16, white bg**, 14px weight 600 black text, 16px icon. Hover darkens the white by 20%.
  - Other actions: 50px transparent ActionIcons.
- **Sticky page header:**
  - 65px high. Appears after scrolling 200px, tinted with the album colour and a 50% black overlay.
  - 48px circular primary play button with an 18px icon; 36px weight-700 title.
- **Context menu:**
  - 256px wide, max-height 576, padding 4, bg `background`, border `border`, radius 5, drop-shadow 0 0 5px rgba(0,0,0,.5).
  - Items: padding 8 12, 13px. With an icon, padding-left 36 and the icon sits at 12.
  - Hover bg `surface`. Selected items get a 2px primary bar. Dividers are 1px `border` with margin 4.
- **Tooltip:** padding 8 12, 16px weight 500, bg `surface`, shadow 2px 2px 10px rgba(0,0,0,.6). Open delay 500ms.

## 6. Home
- Stack gap 32, px 32, pt 48, mb 80.
- **Feature carousel (single):**
  - Radius 5, min-height 240.
  - Background: the blurred image (3rem, opacity .8, scale 1.1) plus a gradient of dominant-colour shades 6→9 at opacity .7.
  - Content padding 24, gap 12, bottom-aligned. 200×200 image, radius 5, with a strong drop shadow.
  - Text is white with a text-shadow: title weight 900 at 28–44px, artist h5 weight 700, genres/year 13px weight 600.
- **Grid carousels:**
  - Title 24px, weight 700, with refresh and prev/next buttons.
  - Gap 12.
  - **Cards per row:** <520: 2, ≥520: 3, ≥720: 4, ≥960: 5, ≥1152: 6, ≥1280: 7, ≥1440: 8.

## 7. Fullscreen player and lyrics
- **Layout:** overlay with an inner max width of 2560px and margin 32 0.
  - Image column 50% (100% with no panel). The panel on the right is 50% with padding 16.
  - Square image, radius 5, drop-shadow 0 5px 15px rgba(0,0,0,.35). Title 3.5vh.
- **Background:** the art, blurred 6rem, under the `overlay-header` gradient and a black layer at 0.208 alpha.
- **Lyrics:**
  - 24px, weight 600, lh 1.2, gap 24.
  - Inactive lines: opacity .2, scale .95. Transition .6s. Line padding 0 16.
  - The reference preset aligns lyrics to the centre.

## 8. Icons (react-icons)
- **Sidebar (Remix, line / fill when active):**
  - Home: RiHome6. Favorites: RiHeart. Albums: RiAlbum. Tracks: RiMusic2.
  - Album Artists and Artists: RiUserVoice. Genres: RiFlag2. Folders: RiFolder3. Playlists: RiPlayList.
  - Now Playing: RiPlay. Radio: RiRadio. Search: RiSearch. Settings: RiSettings2.
- **Player controls (Lucide unless noted):**
  - Transport: LuSquare (stop), LuShuffle, LuSkipBack, LuStepBack, LuPlay, LuPause, LuStepForward, LuSkipForward.
  - Repeat: RiRepeat2Line, RiRepeatOneLine. Shuffle all: RiPlayListAddLine.
  - Right column: LuVolume2/1/X, LuHeart, LuMicVocal, LuPanelRightOpen/Close, LuSlidersHorizontal, LuTimer, LuCast.
  - Other: LuEllipsisVertical, LuChevron*, LuSearch, LuMenu.
- **Placeholders:** album LuDisc3, artist LuUser, song LuMusic, playlist LuListMusic, genre LuFlag.
- **Window controls:** RiSubtractLine, RiCheckboxBlankLine, RiCloseLine.

## 9. Colour derivation
- **Primary palette:** `generateColors(accent)` from `@mantine/colors-generator` creates 10 OKLCH shades.
  - LIGHTNESS_MAP `[.96,.907,.805,.697,.605,.547,.518,.445,.395,.34]`
  - CHROMA_MAP `[.32,.16,.08,.04,0,0,.04,.08,.16,.32]`
  - The input colour sits at the index with the closest L. The other shades take its hue and chroma, set L from the map, then saturate/desaturate by the chroma delta.
- **Where the shades are used:**
  - `--theme-colors-primary` = shade 6.
  - `--theme-colors-primary-filled` = Mantine primaryShade: **dark 5, light 9**.
  - For accent rgb(53,116,252) (which sits at shade 4): shade 5 ≈ `#2764EA`, shade 6 ≈ `#1E5AE0`. **Port the generator; don't hardcode these.**
  - Contrast colour on filled is white.
- **Derived neutrals (dark):**
  - Structural borders: rgba(255,255,255,.05). Content borders: .10. (Light theme borders: rgba(0,0,0,.05).)
  - Player bar bg: `darken(bg,10%)`.
  - Nav hover: `lighten(bg,10%)`. ActionIcon hover: `lighten(bg,5%)`.
  - Default button: bg `surface`, hover `lighten(surface,5%)`, focus `lighten(surface,10%)`.
  - Table row hover: `surface`. Selected: `lighten(surface,5%)`. Odd rows: `darken(bg,30%)`.
  - Transparent-button hover text: `darken(fg,15%)`. Compact card overlay: `alpha(bg,.5)`.
  - Favourite colour: fixed rgb(255,49,49).

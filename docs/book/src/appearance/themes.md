# Themes and colours

This page explains how to change the look of Listen: the theme, light and dark mode, the accent colour, fonts, zoom and a few related options. All of them are on the **Settings > General** tab, in the **Appearance** and **Application** sections. To make your own theme, see [Custom themes](custom-themes.md).

## Choosing a theme

Listen ships with 32 themes plus six desktop presets, and it can load your own. Which picker you see depends on **Follow system theme**:

- **Follow system theme on** (the default): you choose a **Dark theme** and a **Light theme**. Listen uses the one that matches the desktop's light or dark setting and switches when the desktop does. The defaults are Tokyo Night (dark) and Default Light (light).
- **Follow system theme off**: you choose one **Theme** that is always used. The default is Default Dark.

Every picker lists the same themes: the built-in ones, the desktop presets, and your own themes with "(custom)" after their name.

### Built-in themes

| Dark | Light |
| --- | --- |
| Ayu Dark, Catppuccin Mocha, Default Dark, Dracula, Everforest Dark, Github Dark, Glassy Dark, Gruvbox Dark, High Contrast Dark, Material Dark, Monokai, Night Owl, Nord, One Dark, Rose Pine, Rose Pine Moon, Shades Of Purple, Solarized Dark, Tokyo Night, Vscode Dark Plus, Zenburn | Ayu Light, Catppuccin Latte, Default Light, Everforest Light, Github Light, Gruvbox Light, High Contrast Light, Material Light, Rose Pine Dawn, Solarized Light, Vscode Light Plus |

The set is the same as in Feishin, which Listen's interface is modelled on.

## Following the desktop theme

On Linux, **Follow desktop theme** (on by default) makes Listen use the colours of your desktop instead of a built-in theme. It reads the KDE colour scheme (`kdeglobals`, including the accent colour) and follows changes live, usually within a second. When it is on, it takes precedence over the theme choices below it. The row's description tells you what Listen found. If no desktop colours can be found, Listen uses the theme you picked.

The six presets named "(desktop)" in the pickers (Tokyo Night, Catppuccin Mocha, Gruvbox Dark, Nord, Rosé Pine, Dracula) are hand-tuned palettes. Listen selects one of them automatically when a compatible theme-switcher script on your desktop announces it.

This row does not exist on Windows. There, use **Follow system theme**.

## Accent colour and shades

The accent colour is used for the primary buttons, active items and highlights.

- **Use the theme's accent colour**: when on, each theme keeps its own accent. When off (the default), your accent replaces the theme's.
- **Accent colour**: pick one of five swatches, or type any `rgb(r, g, b)` or `#rrggbb` value in the field. The default is `rgb(53, 116, 252)`. The field and swatches are disabled while the previous option is on.
- **Use the theme's primary shade**: when on (the default), buttons and links use the shade the theme prescribes.
- **Primary shade**: when the previous option is off, a slider from 0 (lightest) to 9 (darkest) picks which shade of the accent colour buttons and links use. The default is 6.

## Fonts

| Setting | What it does |
| --- | --- |
| **Font type** | **Built-in** (Inter or Poppins, bundled with Listen), **System** (any font installed on the computer) or **Custom** (a font file). |
| **Font** | The family used for all text. For System fonts, the list fills in after a moment. |
| **Font file** | Custom only. The path to a `.ttf`, `.otf` or `.ttc` file; press **Use** to load it. An error is shown under the field if the file cannot be read. |

The default is the built-in Inter.

## Zoom

**Zoom** scales the whole interface from 50 % to 300 % in steps of 5 %. The default is 100 %. The change applies when you let go of the slider. The **-** and **+** buttons step by 5, **Reset** returns to 100 %. The zoom hotkeys (see [Hotkeys](../integration/hotkeys.md)) change the same value.

## Backgrounds, covers and scrolling

| Setting | What it does | Default |
| --- | --- | --- |
| **Album background image** | Shows the blurred cover behind the header of album pages. The slider sets the blur from 0 to 6. | On, 3.0 |
| **Artist background image** | The same for artist pages. | On, 3.0 |
| **Blur explicit images** | Blurs the covers of songs tagged explicit. | Off |
| **Native cover aspect ratio** | Shows covers whole instead of cropped to a square. | Off |
| **Image placeholder** | What a cover shows while it loads: **ThumbHash** (a blurred preview, available from the second time a cover loads), **Colour** (a tint taken from the cover) or **Off**. | Colour |
| **Touchpad scroll speed** | How far a swipe scrolls, from 25 % to 200 %. At 100 %, scrolling matches Chromium-based browsers for the same swipe. | 100 % |

## Where the values are stored

All of these are stored under `settings.appearance` in `config.toml`. See the [settings reference](../reference/settings.md) for every key.

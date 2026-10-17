# Custom themes

You can add your own colour themes as small text files, and change colours, corner radii, spacing and font sizes of any theme with an override file. Listen reloads them while it runs.

## The themes folder

Custom themes live in the `themes` folder inside Listen's config folder (`~/.config/listen/themes` on Linux). To open it, go to **Settings > General > Appearance** and press **Open folder** on the **Custom themes folder** row. The row also shows how many custom themes were found and whether an override file is active.

Files can be JSON or TOML. Listen notices changes within about a second, so there is no need to restart: save the file and the theme picker and the interface update. Your themes appear in the theme pickers as "Name (custom)".

## A theme file

Create `<name>.json` or `<name>.toml` in the folder. The file name (without the extension) is the theme's id.

```json
{
  "name": "Sunset",
  "extends": "tokyoNight",
  "mode": "dark",
  "colors": {
    "background": "#1d1214",
    "surface": "rgb(42, 26, 29)",
    "primary": "#ff8844"
  }
}
```

The same theme in TOML:

```toml
name = "Sunset"
extends = "tokyoNight"
mode = "dark"

[colors]
background = "#1d1214"
surface = "rgb(42, 26, 29)"
primary = "#ff8844"
```

| Key | Meaning |
| --- | --- |
| `name` | The name shown in the pickers. Defaults to the file name. |
| `extends` | The theme to start from: a built-in theme id (for example `tokyoNight` or `defaultLight`) or another file in the folder. Your colours win over the parent's. Chains can be up to 10 deep; a loop is an error. Without `extends`, the theme starts from `defaultDark`, or from `defaultLight` if `mode` is `light`. |
| `mode` | `dark` or `light`. Defaults to the parent's mode. |
| `colors` | The colours you want to change. |

### Colours

Keys are written in kebab-case:

`background`, `background-alternate`, `foreground`, `foreground-muted`, `primary`, `state-error`, `state-info`, `state-success`, `state-warning`, `surface`, `surface-foreground`

Values can be `#rgb`, `#rrggbb`, `#rrggbbaa` (the alpha part is ignored) or `rgb(r, g, b)`. Colour names such as `blue` are not accepted.

The built-in theme ids are the names in the [themes list](themes.md) written in camel case: `defaultDark`, `catppuccinMocha`, `rosePineDawn`, `vscodeDarkPlus`, `solarizedLight` and so on.

> **Note:** Your accent colour setting still replaces `primary` unless **Use the theme's accent colour** is on.

## The override file

`overrides.json` or `overrides.toml` in the same folder is applied on top of whichever theme is active, including the desktop theme. Use it to tweak things without making a whole theme. Every key is optional.

```toml
[colors]            # the theme colours above, plus:
border = "#ffffff1a"
divider = "#ffffff0d"
nav-hover = "#2a2a3a"
icon-hover = "#26263a"
player-bar = "#101018"
primary-filled = "#3574fc"   # buttons, active navigation
primary-shade = "#2f63d6"    # links, selection outlines

[radius]            # pixels, 0 to 32: sm, md
sm = 3
md = 8

[spacing]           # pixels, 0 to 64: xs sm md lg xl
lg = 20

[font_size]         # pixels, 6 to 48: xs sm md lg xl
md = 15
```

Colours, radii, and the look of buttons, inputs and containers follow the override immediately. Spacing and font sizes apply to every screen.

## When something is wrong

Problems are listed in red or grey under the **Custom themes folder** row in Settings:

- A file that is not valid JSON or TOML, is not an object, or has an unknown `extends` is listed as an error and that theme is skipped.
- A loop in `extends`, or a chain longer than 10, is an error.
- Two files with the same name (for example `sunset.json` and `sunset.toml`) are an error.
- An invalid colour value is dropped with a note, and the rest of the theme still loads. The same goes for unknown colour keys, an unknown `mode`, and out-of-range or non-numeric override values.

Fix the file and save it; the list updates on its own.

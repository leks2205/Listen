# Custom themes and the override file

Listen reads the `themes` folder inside its config directory
(`~/.config/listen/themes` on Linux; Settings > Appearance > "Open folder").
Changes are picked up within a second, no restart needed. Files may be JSON or TOML.
Problems (bad JSON, unknown `extends`, invalid colours) are listed in Settings under
the folder row; invalid colours are dropped and the rest of the theme still loads.

## Theme files: `<name>.json` / `<name>.toml`

The file name is the theme id and appears in the theme pickers as "Name (custom)".

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

- `extends`: a built-in theme id (e.g. `tokyoNight`, `defaultLight`) or another file in the
  folder (chains up to 10 deep; loops are errors). The child's colours win.
  Without `extends` the theme starts from `defaultDark` (or `defaultLight` if `mode` is `light`).
- `mode`: `dark` or `light` (default: taken from the parent).
- `name`: display name (default: the file name).
- `colors` keys (kebab-case, like Feishin): `background`, `background-alternate`, `foreground`,
  `foreground-muted`, `primary`, `state-error`, `state-info`, `state-success`, `state-warning`,
  `surface`, `surface-foreground`. Values: `#rgb`, `#rrggbb`, `#rrggbbaa` (alpha ignored),
  `rgb(r, g, b)`. Colour names are not accepted.

The accent colour setting still replaces `primary` unless "Use the theme's accent colour" is on.

## Override file: `overrides.json` / `overrides.toml`

Applied on top of whichever theme is active (also the desktop theme). It replaces Feishin's
custom CSS. Every key is optional.

```toml
[colors]            # the theme colours above, plus:
border = "#ffffff1a"
divider = "#ffffff0d"
nav-hover = "#2a2a3a"
icon-hover = "#26263a"
player-bar = "#101018"
primary-filled = "#3574fc"   # buttons, active nav
primary-shade = "#2f63d6"    # links, selection outlines

[radius]            # px, 0-32
sm = 3
md = 8

[spacing]           # px, 0-64: xs sm md lg xl
lg = 20

[font_size]         # px, 6-48: xs sm md lg xl
md = 15
```

Radii, colours and the style functions (buttons, inputs, containers) follow the override
immediately. Spacing and font sizes apply to every screen: they are exposed as
`Tokens::space(..)` / `Tokens::font(..)` and mirrored for the views as
`metrics::font_md()`, `metrics::space_lg()`, `metrics::radius_md()` and so on (the
`metrics::FONT_*` / `SPACE_*` / `RADIUS_*` constants are only the defaults).

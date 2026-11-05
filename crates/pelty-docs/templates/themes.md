# Themes

A theme is a folder in `PELTY_THEMES_DIR` (default `extensions/themes`) with a `theme.json`. It can set design tokens, a logo, a stylesheet or a complete frontend bundle (`frontend`), which is then served instead of the default UI.

## theme.json

{{fields:crates/pelty-extensions/src/themes.rs:ThemeManifest}}

## Example (bundled default theme)

{{file:extensions/themes/default/theme.json:json}}

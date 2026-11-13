# Bundled fonts

Font assets are grouped by how Termlet uses them:

- `terminal/source-code-pro/` — Source Code Pro faces embedded for terminal text.
- `icons/` — the generated `my-icons` font, its symbol SVG, and glyph metadata.
  Run `npm run build:icons` to regenerate the icon font from
  `assets/icon-font/`.

The application UI does not bundle a typeface: it uses the platform's default
sans-serif so the app matches the system it runs on. Only the terminal font is
embedded. The embedded files are pulled in at compile time by
`src/core/fonts.rs`; changing their location requires updating that module as
well.

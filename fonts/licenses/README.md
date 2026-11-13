# Font and asset licenses

Termlet embeds two font families plus a set of platform icon glyphs in the
binary. Each is redistributed under its own license, and the license text ships
next to the files it covers.

## Source Code Pro — `terminal/source-code-pro/`

- **Copyright:** 2023 Adobe (http://www.adobe.com/), with Reserved Font Name
  "Source".
- **License:** SIL Open Font License 1.1 — see
  [SourceCodePro-OFL-1.1.txt](SourceCodePro-OFL-1.1.txt).
- **Source:** <https://github.com/adobe-fonts/source-code-pro>
- **Version:** 2.042

## Lucide (icon font) — `icons/`

- **License:** ISC — see [Lucide-ISC.txt](Lucide-ISC.txt).
- **Source:** <https://lucide.dev> via the `lucide-icons` crate.

## Font Awesome Free (icon glyphs) — `assets/icon-font/`

- **License:** CC BY 4.0 (icons) — see
  [FontAwesome-CC-BY-4.0.txt](FontAwesome-CC-BY-4.0.txt).
- **Source:** <https://fontawesome.com>, Font Awesome Free 7.2.0.
- **Used by:** the debian, ubuntu, centos, linux, windows, and macos platform
  glyphs.
- **Attribution:** "Font Awesome Free 7.2.0 by @fontawesome -
  <https://fontawesome.com> License - <https://fontawesome.com/license/free>
  (icons: CC BY 4.0) Copyright 2026 Fonticons, Inc." — the same notice is kept
  in each SVG in `assets/icon-font/`.

`FontAwesome-CC-BY-4.0.txt` is the upstream CC BY 4.0 legal code, downloaded
verbatim from
<https://creativecommons.org/licenses/by/4.0/legalcode.txt> (396 lines;
SHA-256 `9ba9550ad48438d0836ddab3da480b3b69ffa0aac7b7878b5a0039e7ab429411`).
Re-fetch it from that URL rather than editing it by hand.

The `my-icons` font is generated from the SVGs in `assets/icon-font/` with
`npm run build:icons`; it mixes Font Awesome Free glyphs with project-authored
ones.

The application UI itself bundles no typeface and uses the platform default
sans-serif. See [../README.md](../README.md).

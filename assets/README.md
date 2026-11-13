# Assets

Assets are grouped by consumer rather than by file extension:

| Directory | Contents |
| --- | --- |
| `app-icons/` | Application icon masters and platform-ready ICO, ICNS, and PNG files |
| `desktop/` | Linux desktop-entry integration |
| `icon-font/` | SVG sources for the generated `my-icons` UI font |
| `screenshots/` | The README screenshot and GitHub social preview |

## Application icons

`app-icons/` keeps platform-specific files together:

```text
app-icons/
  generic/
    termlet.svg        # shared artwork and runtime icon master
    termlet.png        # 1024 px runtime and fallback icon
    termlet.icns       # generic multi-resolution bundle
  linux/
    256x256/termlet.png
  macos/
    termlet.svg        # Apple HIG safe-area master
    termlet.icns
  windows/
    termlet.svg        # simplified small-size master
    termlet.png
    termlet.ico
    termlet.rc
```

The checked-in ICO, ICNS, and PNG files are derived assets. Regenerate them with
the scripts under `tools/icons/` as described in the root README instead of
editing the binary files directly.

## Icon font

Every SVG in `icon-font/` is compiled into `fonts/icons/my-icons.ttf` by
`npm run build:icons`. Keep that directory limited to glyph sources; application
icons belong in `app-icons/`.

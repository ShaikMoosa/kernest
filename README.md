# Kernest

An open-source font editor for Windows. Draw glyphs, space and kern, interpolate masters and export OTF, TTF and variable fonts — offline, no account.

> **Status: early development (0.1.0).** You can create a font from glyph sets, open the sample font, browse glyphs in Font View and save/open `.kernest` fonts. Drawing, spacing, kerning and export are next.

## Install

Kernest ships as a Windows setup program (`Kernest_<version>_x64-setup.exe`). Run it and choose **Install for me only** (no admin rights needed) or **all users**. It is not distributed through the Microsoft Store, winget or GitHub Releases.

Requirements: Windows 10 22H2 or later (x64 or ARM64). The installer downloads Microsoft Edge WebView2 if it is missing (it is built into Windows 11).

## Build from source

Prerequisites: [Rust](https://rustup.rs) (stable, MSVC toolchain), [Node.js](https://nodejs.org) 22 or later, and Visual Studio Build Tools with the "Desktop development with C++" workload.

```bash
npm install
npm run tauri dev        # run the app with hot reload
cargo test --workspace   # run the Rust tests
npm run tauri build      # build the setup installer into target/release/bundle/nsis/
```

`npm run dev` alone serves the UI in a browser with an in-memory mock backend, which is handy for UI work (file dialogs and saving need the desktop app).

## Project layout

| Path | What it is |
|---|---|
| `crates/kernest-core` | Font data model, outline geometry, glyph data and the `.kernest` file format. No UI dependencies. |
| `src-tauri` | The desktop app shell (Tauri 2): window, file dialogs and the commands the UI calls. |
| `src` | The UI (React + TypeScript). `src/styles/tokens.css` holds the design tokens. |

## The `.kernest` format

A font is a folder, with one JSON file per glyph so changes diff and merge cleanly in Git:

```text
MyFont.kernest/
  font.json      format version, family info, axes, masters, glyph order
  kerning.json   kerning per master
  features.fea   OpenType feature code
  glyphs/
    A_.json      uppercase letters get a "_" so names stay unique on Windows
    a.json
```

Outline nodes are written as short strings such as `"80 0 line"` or `"300 700 curve smooth"`.

## Design

Wireframes, the design system (built on SegmentUI Kit 3.0) and hi-fi screens live in the Kernest Figma file. Design tokens in `src/styles/tokens.css` mirror the Figma variables `Kernest / Color` and `Kernest / Size`.

## Roadmap (v1)

1. Font View and `.kernest` files ✅ (in progress)
2. Edit View: pen tool, nodes, components
3. Spacing and kerning (text mode, kerning window)
4. Masters, axes and interpolation
5. Export OTF, TTF and variable fonts (via [fontc](https://github.com/googlefonts/fontc))
6. Import `.glyphs` / `.glyphspackage` and UFO

Later: smart components, colour fonts, hinting, plug-ins and Python scripting.

## Licence

Apache License 2.0 — see [LICENSE](LICENSE).

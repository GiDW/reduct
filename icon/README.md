---
title: Reduct icon
type: diagram
status: active
created: 2026-11-07
project: [side-projects]
tags: [reduct, icon, windows, msix, design]
---

# Reduct icon

This is the production artwork for the app icon, the "A2 · Tighter" concept: a pale blue rounded square with a smaller, solid blue photo tile overlapping it towards the bottom right. The concept exploration lives in the author's notes, not in this repo. [preview.html](preview.html) shows every asset at real size, on light and dark backgrounds, in a mock context menu and on a mock taskbar.

## Sources (`src/`)

| File | Used for |
| --- | --- |
| `reduct-icon-master.svg` | Everything from 44 px up. It has a 256 grid, Fluent-style layered shadows (ambient plus key light from above), a top-edge highlight, and a two-tone mountain |
| `reduct-icon-16.svg` | 16 px |
| `reduct-icon-20.svg` | 20 px |
| `reduct-icon-24.svg` | 24 px, and anything above 20 and up to 26 px |
| `reduct-icon-32.svg` | 30–40 px |

The small versions are drawn on their own pixel grid:
- no shadows; a 1 px transparent gap separates the two tiles instead
- a slightly more saturated pale tile, so it still shows up on a light theme
- a simplified mountain

## Generated output

Re-run after changing a source, from this folder: `python3 -I tools/render.py`. The script uses only Playwright's headless Chromium, which is already installed on this Mac. It needs no Python packages.

**`assets/` (74 PNGs), named so they drop straight into an MSIX `Images/` folder:**
- `Square44x44Logo.scale-{100,125,150,200,400}`
- `Square44x44Logo.targetsize-{16…256}` (14 sizes), each also as `_altform-unplated` and `_altform-lightunplated`. Windows 11 uses these for the taskbar, Start and the context menu
- `Square71x71Logo`, `Square150x150Logo`, `Square310x310Logo` and `Wide310x150Logo`, each at all five scales. These are Windows 10-style tiles: the icon sits centred, with transparent padding
- `StoreLogo.scale-*`
- `store-listing-300.png`: the square logo for the Partner Center listing
- `master-1024.png`

**`ico/`:**
- `reduct.ico` (16–256 px): the icon of the exe
- `reduct-menu.ico` (16–48 px): the icon `IExplorerCommand::GetIcon` returns for the context menu command

## Manifest references

```xml
<uap:VisualElements DisplayName="Reduct" Description="Optimise images to JPEG"
    BackgroundColor="transparent"
    Square150x150Logo="Images\Square150x150Logo.png"
    Square44x44Logo="Images\Square44x44Logo.png">
  <uap:DefaultTile Wide310x150Logo="Images\Wide310x150Logo.png"
      Square71x71Logo="Images\Square71x71Logo.png"
      Square310x310Logo="Images\Square310x310Logo.png"/>
</uap:VisualElements>
<!-- Properties -->
<Logo>Images\StoreLogo.png</Logo>
```

Windows picks the right `scale-*` or `targetsize-*` file automatically, based on the base name.

## Still to check on a real Windows 11 machine

- **Taskbar and Start:** at 100 %, 125 % and 150 % display scaling, in light and dark mode.
- **Context menu:** the 16 and 20 px versions in Explorer, especially how visible the pale back tile is on the light theme.
- **WACK:** run the Windows App Certification Kit to confirm the asset set is complete.

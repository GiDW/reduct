---
title: Changelog
type: reference
status: active
created: 2026-11-10
project: [side-projects]
tags: [reduct, changelog, releases]
related: [store/README.md]
---

# Changelog

The user-visible changes in each Reduct release. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/). The version is `version` in [Cargo.toml](Cargo.toml). A release is a `v<version>` tag, and the Store package gets `<version>.0`.

Changes that are merged but not released yet go under **Unreleased**. On release, that section becomes the new version, and its summary becomes "What's new in this version" in the Store listings ([store/](store/README.md)).

## [Unreleased]

## [1.1.0] - 2026-11-11

### Added

- Dutch, for the Netherlands and Belgium. Reduct follows the Windows display language: the settings, the progress window and the File Explorer command ("Optimaliseren met Reduct") are in Dutch when Dutch comes before English in the Windows language list.
- Dutch output names: the `Geoptimaliseerd` subfolder and the ` (geoptimaliseerd)` suffix. In Dutch, sizes use a decimal comma (`4,2 MB`).
- Separate US and British English. The United States, Canada and the Philippines get American spelling, and so does any Windows language Reduct doesn't have. The other English regions (the UK, Ireland, Australia, New Zealand, India and more) keep British spelling.

### Changed

- In US English, and for Windows languages Reduct doesn't have, Reduct uses American spelling: **Optimize with Reduct**, "Licenses", "canceled". New outputs go into an `Optimized` subfolder and get an ` (optimized)` suffix. Files already in an `Optimised` folder stay where they are.
- The finished progress window shows **Close** where **Cancel** was, instead of an empty band.
- With HEIC support installed, the settings window is shorter: the bottom buttons move up into the space of the hidden Store button.

### Fixed

- The progress bar fell behind the "N of M done" count on fast batches.
- Uneven spacing below the image size description in the settings window.

## [1.0.0] - 2026-11-08

The first release, in the Microsoft Store.

### Added

- **Optimise with Reduct** in the File Explorer context menu, for JPG, JPEG, JFIF, PNG, WebP, HEIC, HEIF and AVIF.
- JPEG output with mozjpeg, next to the originals or in an `Optimised` subfolder. Originals are never changed or overwritten.
- EXIF orientation applied to the pixels, colours converted to sRGB, all metadata removed and transparent areas flattened onto white or black.
- JPEGs that wouldn't get smaller are skipped.
- Image sizes: original, or a longest side of 3840, 2048, 1280 or 800 px.
- A progress window with a count, a progress bar and **Cancel**. On errors, it offers a failures log and a link to Microsoft's HEVC extension.
- A settings window from the Start menu, with quality, where to save, the name-clash suffix, image size, the background for transparent areas and keeping the original file date.
- x64 and ARM64 builds for Windows 11.

[Unreleased]: https://github.com/GiDW/reduct/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/GiDW/reduct/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/GiDW/reduct/releases/tag/v1.0.0

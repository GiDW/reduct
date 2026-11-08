# Reduct

Reduct is a Windows 11 app that turns images into smaller JPEGs from File Explorer. Select one or more images, right-click and choose **Optimise with Reduct**. The optimised JPEGs are written next to the originals. Reduct never overwrites a file and never changes the originals.

## Supported formats

`jpg`, `jpeg`, `jfif`, `png`, `webp`, `heic`, `heif` and `avif`, in any letter case. Decoding uses the Windows Imaging Component (WIC).

## What the encoder does

- Encodes with mozjpeg at quality 75: progressive, with optimised scans and Huffman tables.
- Strips all metadata.
- Applies the EXIF orientation to the pixels, so photos stay the right way up without metadata.
- Converts colours to sRGB.
- Flattens transparent areas onto a solid background.

When the output name is taken, Reduct adds a suffix: `photo.jpg` becomes `photo_2.jpg`.

When an optimised JPEG would not be smaller than the source JPEG, Reduct writes nothing for that file and counts it as skipped.

## Progress

A batch that takes longer than half a second opens a small progress window with a count, a progress bar, the file being worked on and a **Cancel** button. Cancel finishes the files already being converted and skips the rest. Images already optimised stay.

- **No errors:** the window shows the result for two seconds and closes itself. A batch that finishes within half a second shows nothing.
- **Errors:** the window stays open with the result, **Show details** (the failures log) and, when HEIC support is missing, **Get HEVC extension**.

## Settings

Start Reduct from the Start menu to open its settings.

- **Quality:** JPEG quality from 1 to 100 (default 75).
- **Save to:** next to the original (default), or in an `Optimised` subfolder.
- **When the name is taken:** add `_2`, `_3`, … (default), or add ` (optimised)`.
- **Limit size:** off (default), or scale down to a 2048 or 3840 px long edge.
- **Transparent areas:** flattened onto white (default) or black.
- **Keep the original file date:** copies the original's modified time to the output (default off).

The settings window also shows whether HEIC support is installed and links to the third-party licences.

## HEIC and HEIF

Windows does not decode HEIC out of the box. Reduct uses Microsoft's own codecs from the Microsoft Store:

- **HEIF Image Extensions**: free, and often preinstalled on Windows 11.
- **HEVC Video Extensions**: [Microsoft Store page](https://apps.microsoft.com/detail/9NMZLZ57R3T7), or `ms-windows-store://pdp/?ProductId=9NMZLZ57R3T7` to open it in the Store app. Many PCs already have the free "from Device Manufacturer" version.

When a codec is missing, the settings window and the progress window say so, with a button that opens the Store page.

## Requirements

Windows 11, x64 or ARM64.

## Install

Microsoft Store (link added once published).

## Building

Reduct is built and tested only on GitHub Actions ([.github/workflows/ci.yml](.github/workflows/ci.yml)), natively on an x64 and an ARM64 Windows runner. Every push to `main` produces two artifacts:

- **`reduct-sideload`**: `Reduct_<version>.msixbundle` signed with a throwaway test certificate, that certificate (`reduct-test.cer`) and `INSTALL.txt` with the install steps.
- **`reduct-store`**: `Reduct_<version>.msixupload`, the bundle plus symbols that is uploaded to the Microsoft Store. For a `v*` tag, CI also attaches it to the GitHub Release.

To sideload a CI build on Windows 11:

1. Download `reduct-sideload` from the workflow run and unzip it.
2. In an elevated PowerShell, trust the test certificate: `Import-Certificate .\reduct-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople`
3. Install the bundle: `Add-AppxPackage .\Reduct_<version>.msixbundle`
4. Restart Explorer so the menu entry appears: `Stop-Process -Name explorer -Force`

To uninstall: `Get-AppxPackage Oxyde.Reduct | Remove-AppxPackage`. The `.msixupload` on a release cannot be installed as it is: the Store signs it on publication.

## Repository layout

| Path | Contents |
| --- | --- |
| [crates/reduct](crates/reduct) | `Reduct.exe`: batch runs, the progress window and the settings window |
| [crates/reduct-engine](crates/reduct-engine) | Decoding through WIC, mozjpeg encoding and writing the output files |
| [crates/reduct-common](crates/reduct-common) | Shared logic: supported formats, output naming, settings and the batch list file |
| [crates/reduct-shell](crates/reduct-shell) | `ReductShell.dll`: the File Explorer context-menu command |
| [package](package) | The MSIX manifest and the resource configuration |
| [scripts](scripts) | `package.ps1`, which builds the MSIX packages, the bundle and the `.msixupload` |
| [icon](icon) | Icon sources and the generated assets |

## Licence

MIT, see [LICENSE](LICENSE). Third-party licences are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

## Trademark and privacy

The licence does not cover the name, icon or publisher name: see [TRADEMARK.md](TRADEMARK.md). Reduct collects no personal data: see [PRIVACY.md](PRIVACY.md).

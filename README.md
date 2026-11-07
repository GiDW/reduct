# Reduct

Reduct is a Windows 11 app that optimises images selected in File Explorer to JPEG. Right-click one or more images and choose **Optimise with Reduct**. Originals are never touched or overwritten.

Supported inputs: JPG, JPEG, JFIF, PNG, WebP, HEIC, HEIF and AVIF. Output is a mozjpeg-encoded JPEG with EXIF orientation applied, colours converted to sRGB and metadata stripped.

Reduct is distributed through the Microsoft Store for x64 and ARM64.

## Building

Builds and tests run on GitHub Actions (`.github/workflows/ci.yml`), natively on an x64 and an ARM64 Windows runner. On Windows with the MSVC toolchain:

```
cargo test --workspace
cargo build --workspace --release
```

## Licence

MIT, see [LICENSE](LICENSE).

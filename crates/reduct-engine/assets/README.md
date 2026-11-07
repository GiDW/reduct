---
title: Engine assets
type: reference
status: active
created: 2026-11-07
project: [reduct]
tags: [fixtures, heic]
---

# Engine assets

Files compiled into `reduct-engine` with `include_bytes!`.

- `probe.heic`: the HEIC that `heic_supported()` (`src/codec.rs`) decodes to find out whether WIC can read HEIC on this PC. 64x64, every pixel (200, 100, 50), 487 bytes. Made on macOS: a solid 64x64 RGB PNG written with a small Python PNG writer, then `sips -s format heic probe.png --out probe.heic`. `file` reports "ISO Media, HEIF Image HEVC Main or Main Still Picture Profile". 64x64 rather than smaller so that hardware HEVC decoders with a minimum frame size still accept it.

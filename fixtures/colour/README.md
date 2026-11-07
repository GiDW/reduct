---
title: Colour test fixtures
type: reference
status: active
created: 2026-11-07
project: [reduct]
tags: [fixtures, colour-management]
---

# Colour test fixtures

Tiny images for `crates/reduct-engine/tests/colour.rs`, made on macOS.

- `p3-green-8x8.png`: 8x8 RGB, every pixel (100, 200, 100), written with a small Python PNG writer, then tagged with `sips --embedProfile "/System/Library/ColorSync/Profiles/Display P3.icc"` (embeds the profile without changing the pixels; checked by decoding the PNG data).
- `p3-green-8x8.jpg`: the same pixels saved by `sips -s format jpeg -s formatOptions 100`, then tagged with the Display P3 profile the same way.
- `half-transparent-8x8.png`: 8x8 RGBA, left four columns opaque white (255, 255, 255, 255), right four fully transparent black (0, 0, 0, 0). No profile.
- `grey-gray-profile-8x8.jpg`: 8x8 single-channel grey JPEG, every pixel 128, tagged with the "Generic Gray Gamma 2.2 Profile". An 8-bit greyscale PNG written with the same Python PNG writer, then `sips -s format jpeg -s formatOptions 100` and `sips --embedProfile "/System/Library/ColorSync/Profiles/Generic Gray Gamma 2.2 Profile.icc"`. `sips -g space -g profile` reports `Gray` and the profile name; the JPEG has one component.

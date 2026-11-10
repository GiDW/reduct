---
title: Microsoft Store listings
type: reference
status: active
created: 2026-11-10
project: [side-projects]
tags: [reduct, microsoft-store, store-listing, localisation]
related: [en-US.md, nl.md, ../icon/README.md]
source: https://apps.microsoft.com/detail/9n12xrp8dn9v
---

# Microsoft Store listings

The text for every Store listing of Reduct, one file per language. Copy each field into Partner Center: **Apps and games → Reduct → Store listings → (language)**.

| Partner Center language | File |
| --- | --- |
| English (United States), `en-US` | [en-US.md](en-US.md) |
| Dutch (Netherlands), `nl-NL` | [nl.md](nl.md) |
| Dutch (Belgium), `nl-BE` | [nl.md](nl.md) (the same text as `nl-NL`) |

Partner Center only offers a listing language when the package declares it. The languages are the `<Resources>` in [package/AppxManifest.xml](../package/AppxManifest.xml). The Dutch listings appear once a package with `nl-NL` and `nl-BE` is submitted (version 1.1.0 or later). The app's own UI text is in [crates/reduct-common/src/lang.rs](../crates/reduct-common/src/lang.rs). Keep the listing in step with it, for example the `Optimise with Reduct` and `Optimaliseren met Reduct` menu names.

## Fields and limits

| Field | Limit |
| --- | --- |
| Description | 10,000 characters |
| What's new in this version | 1,500 characters |
| Product features | up to 20, 200 characters each |
| Short description | 1,000 characters (the Store shows about the first 270) |
| Screenshot captions | 200 characters each |
| Search terms | up to 7, 30 characters each, 21 words in total |

## Artwork

These come from [icon/assets](../icon/assets). They are the same for every language unless a listing overrides them.

- **1:1 box art:** `store-boxart-2160.png`. Without it, the store page shows the 100 px logo from the package, which looks blurry.
- **1:1 app tile icon:** `store-listing-300.png`.
- **Screenshots:** 1920 × 1080. The Dutch listings need screenshots of the Dutch UI: Windows set to Dutch, with the 1.1.0 build.

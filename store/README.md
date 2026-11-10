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

The text for every Store listing of Reduct, one file per language. These files are the source of truth: for a `v*` tag, CI's `store` job ([scripts/store_submission.py](../scripts/store_submission.py)) submits the package together with every listing, screenshot and logo below, after the maintainer approves the `microsoft-store` deployment. Don't edit the listings in Partner Center: the next release overwrites them.

The script reads each file by its `## ` headings and the ```` ```text ```` block under each, the caption table by its rows (`| # | `file` | `caption` |`), and the `For <version>.` note above "What's new". Keep that layout. The `store-listing` CI job checks it, and the limits below, on every push. It accepts a "What's new" marked for a version later than `Cargo.toml`, because the text is written before the release commit bumps the version. The `store` job needs an exact match.

| Partner Center language | File |
| --- | --- |
| English (United States), `en-US` | [en-US.md](en-US.md) (American spelling) |
| English (United Kingdom), `en-GB` | [en-GB.md](en-GB.md) (British spelling) |
| Dutch (Netherlands), `nl-NL` | [nl.md](nl.md) |
| Dutch (Belgium), `nl-BE` | [nl.md](nl.md) (the same text as `nl-NL`) |

The Store only accepts a listing language when the package declares it. The languages are the `<Resources>` in [package/AppxManifest.xml](../package/AppxManifest.xml). To add a language, add it there, add its listing file, and map it in `LANGUAGES` in the script. The app's own UI text is in [crates/reduct-common/src/lang.rs](../crates/reduct-common/src/lang.rs). Keep the listing in step with it, for example the `Optimize with Reduct`, `Optimise with Reduct` and `Optimaliseren met Reduct` menu names.

## Automated submission: one-time setup

The `store` job in [.github/workflows/ci.yml](../.github/workflows/ci.yml) needs:

1. **An Entra ID app in Partner Center.** Account settings → User management → Microsoft Entra applications → create one (or add an existing one) with the **Manager** role, then add a key. Note the tenant ID, client ID and key. The key expires (at most after two years): renew it before then and update the secret.
2. **A GitHub environment `microsoft-store`** (Settings → Environments):
   - Required reviewers: the maintainer. Leave "Prevent self-review" off, so the maintainer can approve their own release.
   - Deployment branches and tags: selected, with the tag rule `v*`.
   - Environment secrets: `STORE_TENANT_ID`, `STORE_CLIENT_ID`, `STORE_CLIENT_SECRET`.

Create the environment before the first tag. When a job names an environment that doesn't exist, GitHub creates it without protection rules, and the job would run without waiting for approval (and fail for lack of secrets).

The API needs the app to have one submission completed in Partner Center first, which Reduct has. Once a submission is made by the API, change it only through the API: editing it in Partner Center leaves it stuck.

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

The logos come from [icon/assets](../icon/assets). Every listing gets the same artwork.

- **1:1 box art:** `store-boxart-2160.png`. Without it, the store page shows the 100 px logo from the package, which looks blurry.
- **1:1 app tile icon:** `store-listing-300.png`.
- **Screenshots:** [screenshots/](screenshots/), 1920 × 1080, numbered in listing order. They show the British English UI and are used for every listing, the US and Dutch ones too, so captions only quote UI text where it matches. The caption tables in each language file use the same numbers.

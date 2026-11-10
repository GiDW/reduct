#!/usr/bin/env python3
"""Microsoft Store submission for Reduct, from the listings in store/*.md.

  store_submission.py check
      Parses every listing file and checks the Store's field limits and the
      screenshot files, and that "What's new" is marked for the version in
      Cargo.toml or a later one (notes are written before the version bump).
      No network. CI runs it on every push.

  store_submission.py submit --package Reduct_<v>.0.msixupload
      Does the same checks, but "What's new" must be marked for exactly the
      version in Cargo.toml. Then it creates a Store submission (a copy of the
      last published one), replaces its packages, listings, screenshots and
      logos, uploads the files, commits it and waits until the Store has
      accepted it for certification. Needs the environment variables
      STORE_TENANT_ID, STORE_CLIENT_ID, STORE_CLIENT_SECRET and STORE_APP_ID.

Standard library only, so it runs on any CI runner without installs.
API reference: https://learn.microsoft.com/windows/uwp/monetize/manage-app-submissions
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import sys
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STORE = ROOT / "store"
API = "https://manage.devcenter.microsoft.com/v1.0/my"

# Store listing language (as the API spells it) -> listing file.
LANGUAGES = {
    "en-us": "en-US.md",
    "en-gb": "en-GB.md",
    "nl-nl": "nl.md",
    "nl-be": "nl.md",
}

# Artwork shared by every listing: zip path -> (file, API image type).
LOGOS = {
    "images/store-boxart-2160.png": (ROOT / "icon/assets/store-boxart-2160.png", "StoreLogoSquare"),
    "images/store-listing-300.png": (ROOT / "icon/assets/store-listing-300.png", "Icon"),
}

# The image types this script owns. Images of other types in the
# submission (if someone adds them in Partner Center) are left alone.
MANAGED_IMAGE_TYPES = {"Screenshot"} | {kind for _, kind in LOGOS.values()}

# Partner Center limits, the same as the table in store/README.md.
MAX_DESCRIPTION = 10_000
MAX_RELEASE_NOTES = 1_500
MAX_FEATURES = 20
MAX_FEATURE = 200
MAX_SHORT_DESCRIPTION = 1_000
MAX_CAPTION = 200
MAX_SCREENSHOTS = 10
MAX_KEYWORDS = 7
MAX_KEYWORD = 30
MAX_KEYWORD_WORDS = 21

# Fields left out of the submission sent back to the API: the ones it fills
# in itself, and `pricing`. Reduct is on Pricing Version 2 (Partner Center
# shows "Review price per market"), for which the API returns the price as an
# unknown tier. Sending that back could change the price, so the price is
# managed in Partner Center only.
OMITTED_FIELDS = ("id", "status", "statusDetails", "fileUploadUrl", "pricing")

# A single PUT to the upload URL is limited to 64 MB at the SAS API version
# the Store hands out (sv=2014-02-14).
MAX_UPLOAD = 64 * 1024 * 1024


class Failure(Exception):
    pass


class Rejected(Failure):
    """The Store processed the commit and turned the submission down."""


@dataclass
class Listing:
    path: Path
    title: str
    description: str
    release_notes: str
    release_notes_version: str
    features: list[str]
    short_description: str
    screenshots: list[tuple[str, str]]  # (file name in store/screenshots, caption)
    keywords: list[str]
    copyright: str
    problems: list[str] = field(default_factory=list)


# --- Listing files -----------------------------------------------------------


def sections(text: str) -> dict[str, str]:
    """Splits a listing file into its `## ` sections, by heading."""
    out: dict[str, str] = {}
    current = None
    lines: list[str] = []
    for line in text.splitlines():
        if line.startswith("## "):
            if current is not None:
                out[current] = "\n".join(lines)
            current = line[3:].strip()
            lines = []
        elif current is not None:
            lines.append(line)
    if current is not None:
        out[current] = "\n".join(lines)
    return out


def text_block(section: str) -> str | None:
    """The contents of the first ```text block in a section."""
    m = re.search(r"^```text\n(.*?)\n```$", section, re.S | re.M)
    return m.group(1).strip("\n") if m else None


def parse_listing(path: Path) -> Listing:
    parts = sections(path.read_text(encoding="utf-8"))
    problems: list[str] = []

    def block(heading: str) -> str:
        if heading not in parts:
            problems.append(f"no '## {heading}' section")
            return ""
        value = text_block(parts[heading])
        if value is None:
            problems.append(f"'## {heading}' has no ```text block")
            return ""
        return value

    def lines(heading: str) -> list[str]:
        return [l.strip() for l in block(heading).splitlines() if l.strip()]

    whats_new = parts.get("What's new in this version", "")
    m = re.search(r"^For (\d+\.\d+\.\d+)\.\s*$", whats_new, re.M)
    if whats_new and not m:
        problems.append("'## What's new in this version' has no 'For <version>.' line")

    screenshots = []
    caption_section = parts.get("Screenshot captions")
    if caption_section is None:
        problems.append("no '## Screenshot captions' section")
    else:
        for row in re.finditer(r"^\|\s*(\d+)\s*\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|\s*$", caption_section, re.M):
            screenshots.append((row.group(2), row.group(3)))

    return Listing(
        path=path,
        title=block("Product name"),
        description=block("Description"),
        release_notes=block("What's new in this version"),
        release_notes_version=m.group(1) if m else "",
        features=lines("Product features"),
        short_description=block("Short description"),
        screenshots=screenshots,
        keywords=lines("Search terms"),
        copyright=block("Copyright and trademark info"),
        problems=problems,
    )


def version_key(v: str) -> tuple[int, ...]:
    return tuple(int(x) for x in v.split("."))


def check_listing(listing: Listing, version: str, exact: bool) -> list[str]:
    p = list(listing.problems)

    def limit(name: str, value: str, most: int) -> None:
        if not value:
            p.append(f"{name} is empty")
        elif len(value) > most:
            p.append(f"{name} is {len(value)} characters, the limit is {most}")

    limit("Product name", listing.title, 256)
    limit("Description", listing.description, MAX_DESCRIPTION)
    limit("What's new", listing.release_notes, MAX_RELEASE_NOTES)
    limit("Short description", listing.short_description, MAX_SHORT_DESCRIPTION)

    notes = listing.release_notes_version
    if notes and (notes != version if exact else version_key(notes) < version_key(version)):
        p.append(
            f"What's new is marked 'For {notes}.', but Cargo.toml is {version}: "
            "update the text and the note for this version"
        )

    if not listing.features:
        p.append("no product features")
    if len(listing.features) > MAX_FEATURES:
        p.append(f"{len(listing.features)} product features, the limit is {MAX_FEATURES}")
    for f in listing.features:
        if len(f) > MAX_FEATURE:
            p.append(f"feature is {len(f)} characters, the limit is {MAX_FEATURE}: {f[:40]}…")

    if not listing.keywords:
        p.append("no search terms")
    if len(listing.keywords) > MAX_KEYWORDS:
        p.append(f"{len(listing.keywords)} search terms, the limit is {MAX_KEYWORDS}")
    for k in listing.keywords:
        if len(k) > MAX_KEYWORD:
            p.append(f"search term is {len(k)} characters, the limit is {MAX_KEYWORD}: {k}")
    words = sum(len(k.split()) for k in listing.keywords)
    if words > MAX_KEYWORD_WORDS:
        p.append(f"search terms have {words} words, the limit is {MAX_KEYWORD_WORDS}")

    if not listing.screenshots:
        p.append("no screenshot captions")
    if len(listing.screenshots) > MAX_SCREENSHOTS:
        p.append(f"{len(listing.screenshots)} screenshots, the limit is {MAX_SCREENSHOTS}")
    for name, caption in listing.screenshots:
        if not (STORE / "screenshots" / name).is_file():
            p.append(f"screenshot {name} is not in store/screenshots/")
        if len(caption) > MAX_CAPTION:
            p.append(f"caption for {name} is {len(caption)} characters, the limit is {MAX_CAPTION}")
    return p


def cargo_version() -> str:
    data = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    return data["workspace"]["package"]["version"]


def load_listings(version: str, exact: bool) -> dict[str, Listing]:
    """Parses and checks every listing file; raises with all problems found."""
    listings: dict[str, Listing] = {}
    problems: list[str] = []
    for name in sorted(set(LANGUAGES.values())):
        listing = parse_listing(STORE / name)
        listings[name] = listing
        problems += [f"store/{name}: {msg}" for msg in check_listing(listing, version, exact)]
    for path, _ in LOGOS.values():
        if not path.is_file():
            problems.append(f"{path.relative_to(ROOT)} is missing")
    # Every listing shares the same screenshots, in the same order.
    orders = {name: [s for s, _ in l.screenshots] for name, l in listings.items()}
    if len({tuple(o) for o in orders.values()}) > 1:
        problems.append(f"the listing files list different screenshots: {orders}")
    notes = {name: l.release_notes_version for name, l in listings.items()}
    if len(set(notes.values())) > 1:
        problems.append(f"the listing files mark What's new for different versions: {notes}")
    if problems:
        raise Failure("listing check failed:\n  " + "\n  ".join(problems))
    return listings


# --- Store submission API -----------------------------------------------------


def env(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise Failure(f"environment variable {name} is not set")
    return value


IDEMPOTENT = {"GET", "PUT", "DELETE"}


def request(method: str, url: str, token: str | None = None, body=None, headers=None, raw: bytes | None = None):
    """One API call. Only GET, PUT and DELETE are retried: a POST that seemed
    to fail may have gone through, and the callers check for that."""
    hdrs = dict(headers or {})
    data = raw
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        hdrs["Content-Type"] = "application/json"
    if token:
        hdrs["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, data=data, method=method, headers=hdrs)
    tries = 4 if method in IDEMPOTENT else 1
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(req, timeout=300) as resp:
                payload = resp.read()
                if not payload:
                    return None
                try:
                    return json.loads(payload)
                except json.JSONDecodeError:
                    return payload.decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            detail = e.read().decode("utf-8", "replace")
            # 429 and 5xx are worth another try; the rest are our mistake.
            if (e.code == 429 or e.code >= 500) and attempt < tries - 1:
                wait = 10 * (attempt + 1)
                print(f"{method} {redact(url)}: HTTP {e.code}, retrying in {wait} s", flush=True)
                time.sleep(wait)
                continue
            raise Failure(f"{method} {redact(url)}: HTTP {e.code}\n{detail}") from None
        except (urllib.error.URLError, OSError) as e:
            # URLError for connection problems, TimeoutError (an OSError)
            # for a response that stalls.
            if attempt < tries - 1:
                time.sleep(10 * (attempt + 1))
                continue
            raise Failure(f"{method} {redact(url)}: {getattr(e, 'reason', e)}") from None
    raise AssertionError("unreachable")


def redact(url: str) -> str:
    """Drops the query string, which holds the upload URL's SAS signature."""
    return url.split("?", 1)[0]


class Token:
    """An Entra ID token, renewed before its 60 minutes run out."""

    def __init__(self) -> None:
        self._value = ""
        self._fetched = 0.0

    def __call__(self) -> str:
        if not self._value or time.monotonic() - self._fetched > 45 * 60:
            self._value = access_token()
            self._fetched = time.monotonic()
        return self._value


def access_token() -> str:
    tenant = env("STORE_TENANT_ID")
    form = urllib.parse.urlencode(
        {
            "grant_type": "client_credentials",
            "client_id": env("STORE_CLIENT_ID"),
            "client_secret": env("STORE_CLIENT_SECRET"),
            "resource": "https://manage.devcenter.microsoft.com",
        }
    ).encode()
    result = request(
        "POST",
        f"https://login.microsoftonline.com/{urllib.parse.quote(tenant)}/oauth2/token",
        raw=form,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    token = result.get("access_token") if isinstance(result, dict) else None
    if not token:
        raise Failure("the token response has no access_token")
    return token


def build_submission(submission: dict, listings: dict[str, Listing], package_name: str) -> dict:
    """Replaces the packages and the listings in a submission copied from the last one."""
    for pkg in submission.get("applicationPackages") or []:
        pkg["fileStatus"] = "PendingDelete"
    submission.setdefault("applicationPackages", []).append(
        {
            "fileName": package_name,
            "fileStatus": "PendingUpload",
            "minimumDirectXVersion": "None",
            "minimumSystemRam": "None",
        }
    )

    all_listings = submission.get("listings") or {}
    submission["listings"] = all_listings
    # Listings in languages we don't maintain would keep stale text.
    unknown = sorted(k for k in all_listings if k.lower() not in LANGUAGES)
    if unknown:
        raise Failure(f"the submission has listings this script doesn't maintain: {', '.join(unknown)}")
    # Keep the API's own spelling of an existing key.
    keys = {k.lower(): k for k in all_listings}

    for lang, file_name in LANGUAGES.items():
        src = listings[file_name]
        entry = all_listings.setdefault(keys.get(lang, lang), {}) or {}
        all_listings[keys.get(lang, lang)] = entry
        base = entry.setdefault("baseListing", {}) or {}
        entry["baseListing"] = base
        base.update(
            {
                "title": src.title,
                "description": src.description,
                "releaseNotes": src.release_notes,
                "features": src.features,
                "shortDescription": src.short_description,
                "keywords": src.keywords,
                "copyrightAndTrademarkInfo": src.copyright,
            }
        )
        images = []
        for image in base.get("images") or []:
            if image.get("imageType") in MANAGED_IMAGE_TYPES:
                image["fileStatus"] = "PendingDelete"
            images.append(image)
        for name, caption in src.screenshots:
            images.append(
                {
                    "fileName": f"images/screenshots/{name}",
                    "fileStatus": "PendingUpload",
                    "imageType": "Screenshot",
                    "description": caption,
                }
            )
        for zip_name, (_, kind) in LOGOS.items():
            images.append({"fileName": zip_name, "fileStatus": "PendingUpload", "imageType": kind})
        base["images"] = images
    return submission


def build_zip(package: Path, listings: dict[str, Listing]) -> bytes:
    """One copy of each file; every listing refers to the shared images."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        # The package is already compressed: store it.
        z.write(package, package.name, compress_type=zipfile.ZIP_STORED)
        names = next(iter(listings.values())).screenshots
        for name, _ in names:
            z.write(STORE / "screenshots" / name, f"images/screenshots/{name}")
        for zip_name, (path, _) in LOGOS.items():
            z.write(path, zip_name)
    return buf.getvalue()


def submit(package: Path, version: str, listings: dict[str, Listing]) -> None:
    app_id = env("STORE_APP_ID")
    expected = f"Reduct_{version}.0.msixupload"
    if package.name != expected:
        raise Failure(f"package is {package.name}, expected {expected} for version {version}")
    if not package.is_file():
        raise Failure(f"{package} does not exist")

    archive = build_zip(package, listings)
    print(f"Upload archive: {len(archive) / 1024 / 1024:.1f} MB", flush=True)
    if len(archive) > MAX_UPLOAD:
        raise Failure(f"the upload archive is {len(archive)} bytes, over the {MAX_UPLOAD} byte limit of one upload")

    token = Token()
    apps = f"{API}/applications/{urllib.parse.quote(app_id)}"

    def pending_id() -> str | None:
        app = request("GET", apps, token()) or {}
        return (app.get("pendingApplicationSubmission") or {}).get("id")

    pending = pending_id()
    if pending:
        raise Failure(
            f"the app already has a submission in progress ({pending}). "
            "Finish or delete it in Partner Center (Apps and games → Reduct → the submission → Delete), "
            "then re-run this job."
        )

    try:
        submission = request("POST", f"{apps}/submissions", token())
    except Failure as e:
        # The create may have gone through even though the answer was lost.
        orphan = None
        try:
            orphan = pending_id()
        except Failure:
            pass
        if orphan:
            raise Failure(f"{e}\nSubmission {orphan} may have been created anyway: delete it in Partner Center.") from None
        raise
    sid = submission["id"]
    upload_url = submission["fileUploadUrl"]
    print(f"Created submission {sid}", flush=True)
    committed = False
    try:
        build_submission(submission, listings, package.name)
        body = {k: v for k, v in submission.items() if k not in OMITTED_FIELDS}
        try:
            request("PUT", f"{apps}/submissions/{sid}", token(), body=body)
        except Failure as e:
            if "pricing" in str(e).lower():
                raise Failure(
                    f"{e}\nThe API wants the pricing section, which this script leaves out on purpose: "
                    "Reduct is on Pricing Version 2, for which the API can't read the price. "
                    "See store/README.md."
                ) from None
            raise
        print("Updated the packages and the listings", flush=True)

        request("PUT", upload_url, raw=archive, headers={"x-ms-blob-type": "BlockBlob"})
        print("Uploaded the archive", flush=True)

        try:
            request("POST", f"{apps}/submissions/{sid}/commit", token())
        except Failure:
            # The commit may have gone through even though the answer was
            # lost: only a submission still waiting for its commit wasn't.
            try:
                state = (request("GET", f"{apps}/submissions/{sid}/status", token()) or {}).get("status")
            except Failure:
                state = None
            if state != "PendingCommit":
                committed = True
            raise
        committed = True
        print("Committed; waiting for the Store to process it", flush=True)
        wait_for_commit(apps, sid, token)
    except BaseException as e:
        # Leave no half-made submission behind: it would block the next run.
        # A committed submission the Store rejected is deleted too, after its
        # errors have been printed, so that re-running the job starts clean.
        # One whose outcome is unknown (a lost commit answer, a timeout, a
        # cancelled job) is kept: it may yet go through.
        if committed and not isinstance(e, Rejected):
            print(f"::warning::submission {sid} may be committed but its outcome is unknown; check Partner Center", flush=True)
            raise
        try:
            request("DELETE", f"{apps}/submissions/{sid}", token())
            print(f"Deleted submission {sid}", flush=True)
        except Failure as err:
            print(f"::warning::could not delete submission {sid}; delete it in Partner Center: {err}", flush=True)
        raise
    print(f"Submission {sid} is in certification. Follow it in Partner Center.")


# The Store has accepted the commit once the status reaches one of these.
ACCEPTED = {"PreProcessing", "Certification", "Release", "PendingPublication", "Publishing", "Published"}
# Still on its way.
WAITING = {"PendingCommit", "CommitStarted"}


def wait_for_commit(apps: str, sid: str, token: Token) -> None:
    started = time.monotonic()
    while True:
        if time.monotonic() - started > 50 * 60:
            raise Failure("the Store did not finish processing the commit within 50 minutes")
        time.sleep(30)
        status = request("GET", f"{apps}/submissions/{sid}/status", token()) or {}
        state = status.get("status")
        details = status.get("statusDetails") or {}
        print(f"Status: {state}", flush=True)
        if state in WAITING:
            continue
        for w in details.get("warnings") or []:
            print(f"::warning::{w.get('code')}: {w.get('details')}", flush=True)
        if state in ACCEPTED:
            return
        errors = "\n  ".join(f"{e.get('code')}: {e.get('details')}" for e in details.get("errors") or [])
        message = f"the submission ended in status {state}:\n  {errors or '(no details)'}"
        if state == "Canceled" or (state or "").endswith("Failed"):
            raise Rejected(message)
        # A status this script doesn't know: don't delete what may be fine.
        raise Failure(message)


# --- Command line -------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check", help="check the listing files, without network")
    s = sub.add_parser("submit", help="create, fill and commit a Store submission")
    s.add_argument("--package", type=Path, required=True, help="the .msixupload to submit")
    args = parser.parse_args()

    try:
        version = cargo_version()
        listings = load_listings(version, exact=args.command == "submit")
        for lang, name in LANGUAGES.items():
            l = listings[name]
            print(
                f"{lang} (store/{name}): {len(l.description)} characters of description, "
                f"{len(l.features)} features, {len(l.screenshots)} screenshots, {len(l.keywords)} search terms"
            )
        if args.command == "submit":
            submit(args.package, version, listings)
        else:
            print(f"Listings OK (Cargo.toml is {version})")
    except Failure as e:
        print(f"::error::{e}".replace("\n", "%0A"), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Render the Reduct icon SVGs to the MSIX PNG asset set and to .ico files.

Uses Playwright's headless Chromium (no other dependencies): the SVGs are drawn
onto canvases in a generated page, read back with --dump-dom, and written out.

Usage: python3 -I tools/render.py   (run from the final/ folder)
"""
import base64, glob, json, os, re, struct, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SRC = os.path.join(ROOT, "src")
ASSETS = os.path.join(ROOT, "assets")
ICO = os.path.join(ROOT, "ico")

def chromium():
    hits = sorted(glob.glob(os.path.expanduser(
        "~/Library/Caches/ms-playwright/chromium_headless_shell-*/*/chrome-headless-shell")))
    if not hits:
        sys.exit("Playwright headless Chromium not found (npx playwright install chromium-headless-shell)")
    return hits[-1]

def source_for(px):
    """Hand-tuned art for small sizes, the master for everything from 44 px up."""
    if px <= 16: return "reduct-icon-16.svg"
    if px <= 20: return "reduct-icon-20.svg"
    if px <= 26: return "reduct-icon-24.svg"
    if px < 44:  return "reduct-icon-32.svg"
    return "reduct-icon-master.svg"

# (file name, canvas w, canvas h, icon size in px)
jobs = []
def square_full(name, n): jobs.append((name, n, n, n))
def tile(name, w, h, frac): jobs.append((name, w, h, round(min(w, h) * frac)))

scales = [100, 125, 150, 200, 400]
for s in scales:
    square_full(f"Square44x44Logo.scale-{s}.png", round(44 * s / 100))
    square_full(f"StoreLogo.scale-{s}.png", round(50 * s / 100))
    tile(f"Square71x71Logo.scale-{s}.png", round(71 * s / 100), round(71 * s / 100), 0.6)
    tile(f"Square150x150Logo.scale-{s}.png", round(150 * s / 100), round(150 * s / 100), 0.6)
    tile(f"Square310x310Logo.scale-{s}.png", round(310 * s / 100), round(310 * s / 100), 0.5)
    tile(f"Wide310x150Logo.scale-{s}.png", round(310 * s / 100), round(150 * s / 100), 0.6)
for t in [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]:
    for suffix in ["", "_altform-unplated", "_altform-lightunplated"]:
        square_full(f"Square44x44Logo.targetsize-{t}{suffix}.png", t)
square_full("store-listing-300.png", 300)
# Partner Center store logos: the 1:1 box art (2160 recommended, 1080 minimum).
square_full("store-boxart-2160.png", 2160)
square_full("master-1024.png", 1024)

ico_sets = {
    "reduct.ico": [16, 20, 24, 32, 40, 48, 64, 96, 128, 256],       # exe / app icon
    "reduct-menu.ico": [16, 20, 24, 32, 40, 48],                    # context menu command
}
for sizes in ico_sets.values():
    for n in sizes:
        square_full(f"__ico_{n}.png", n)

def data_url(fname):
    with open(os.path.join(SRC, fname), "rb") as f:
        return "data:image/svg+xml;base64," + base64.b64encode(f.read()).decode()

srcs = {f: data_url(f) for f in {source_for(j[3]) for j in jobs}}
page = """<!DOCTYPE html><html><body><pre id="out"></pre><script>
const SRCS = %s, JOBS = %s;
const load = u => new Promise(r => { const i = new Image(); i.onload = () => r(i); i.src = u; });
(async () => {
  const imgs = {}; for (const k in SRCS) imgs[k] = await load(SRCS[k]);
  const out = {};
  for (const [name, w, h, icon, src] of JOBS) {
    const c = document.createElement('canvas'); c.width = w; c.height = h;
    const g = c.getContext('2d'); g.imageSmoothingQuality = 'high';
    g.drawImage(imgs[src], Math.round((w - icon) / 2), Math.round((h - icon) / 2), icon, icon);
    out[name] = c.toDataURL('image/png');
  }
  document.getElementById('out').textContent = JSON.stringify(out);
})();
</script></body></html>""" % (json.dumps(srcs), json.dumps([list(j) + [source_for(j[3])] for j in jobs]))

with tempfile.TemporaryDirectory() as tmp:
    p = os.path.join(tmp, "render.html")
    open(p, "w").write(page)
    dom = subprocess.run([chromium(), "--virtual-time-budget=20000", "--dump-dom", "file://" + p],
                         capture_output=True, text=True).stdout
m = re.search(r'<pre id="out">(.*?)</pre>', dom, re.S)
if not m or not m.group(1).strip():
    sys.exit("render failed: no output from Chromium")
out = json.loads(m.group(1).replace("&quot;", '"').replace("&amp;", "&"))

os.makedirs(ASSETS, exist_ok=True); os.makedirs(ICO, exist_ok=True)
pngs = {}
for name, url in out.items():
    data = base64.b64decode(url.split(",", 1)[1])
    if name.startswith("__ico_"):
        pngs[int(name[6:-4])] = data
    else:
        open(os.path.join(ASSETS, name), "wb").write(data)

for ico, sizes in ico_sets.items():
    header = struct.pack("<HHH", 0, 1, len(sizes))
    entries, blobs, offset = b"", b"", 6 + 16 * len(sizes)
    for n in sizes:
        d = pngs[n]
        entries += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(d), offset)
        blobs += d; offset += len(d)
    open(os.path.join(ICO, ico), "wb").write(header + entries + blobs)

print(f"{len([n for n in out if not n.startswith('__')])} PNG assets, {len(ico_sets)} .ico files")

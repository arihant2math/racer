"""Fetch reference photos from Wikimedia Commons with license metadata.

usage: python3 pipelines/refs/commons_fetch.py OUT_DIR "File:A.jpg" "File:B.jpg" ...
       python3 pipelines/refs/commons_fetch.py OUT_DIR --list refs.txt

Downloads a 1024 px thumbnail of each file and appends author, license and
source URL to OUT_DIR/SOURCES.tsv. Polite: one request at a time, retries on 429.
"""
import json
import os
import sys
import time
import urllib.parse
import urllib.request

API = "https://commons.wikimedia.org/w/api.php"
UA = "racer-asset-pipeline/0.1 (https://github.com/arihant2math/racer)"
WIDTH = 1024


def get(url, binary=False):
    for attempt in range(10):
        req = urllib.request.Request(url, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                data = r.read()
                return data if binary else json.loads(data)
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(15 * (attempt + 1))
                continue
            raise
    raise RuntimeError(f"rate limited: {url}")


def strip_html(s):
    import re
    return re.sub(r"<[^>]+>", "", s or "").strip()


def main():
    out = sys.argv[1]
    titles = sys.argv[2:]
    if titles and titles[0] == "--list":
        titles = [l.strip() for l in open(titles[1]) if l.strip() and not l.startswith("#")]
    os.makedirs(out, exist_ok=True)
    sources = os.path.join(out, "SOURCES.tsv")
    have = set()
    if os.path.exists(sources):
        have = {l.split("\t")[0] for l in open(sources)}
    else:
        with open(sources, "w") as f:
            f.write("file\ttitle\tauthor\tlicense\tsource\n")
    for t in titles:
        q = urllib.parse.urlencode({
            "action": "query", "titles": t, "prop": "imageinfo", "format": "json",
            "iiprop": "url|extmetadata", "iiurlwidth": WIDTH,
        })
        page = next(iter(get(f"{API}?{q}")["query"]["pages"].values()))
        if "imageinfo" not in page:
            print("missing", t)
            continue
        ii = page["imageinfo"][0]
        meta = ii.get("extmetadata", {})
        name = t.split(":", 1)[1].replace(" ", "_")
        name = os.path.splitext(name)[0] + ".jpg"
        if name in have:
            continue
        data = get(ii["thumburl"], binary=True)
        with open(os.path.join(out, name), "wb") as f:
            f.write(data)
        author = strip_html(meta.get("Artist", {}).get("value"))
        lic = meta.get("LicenseShortName", {}).get("value", "?")
        with open(sources, "a") as f:
            f.write(f"{name}\t{t}\t{author}\t{lic}\t{ii['descriptionurl']}\n")
        print("ok", name, lic)
        time.sleep(8.0)


if __name__ == "__main__":
    main()

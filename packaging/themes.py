#!/usr/bin/env python3
"""Fetches the VS Code themes the editor bundles and writes each as one plain
JSON file under crates/yara-core/assets/themes, with any `include` chain
folded in and the comments and trailing commas theme files carry taken out,
so the editor can read them with serde alone. Dark Modern stays hand-written
in theme.rs. `make themes` runs it."""

import io
import json
import os
import re
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "crates", "yara-core", "assets", "themes")
OPEN_VSX = "https://open-vsx.org/api/"

# (name the picker shows, file it is written to, where it comes from): a raw
# URL, or an Open VSX extension and the file inside its package for themes
# that are only built at packaging time. Every source is MIT.
THEMES = [
    (
        "Monokai",
        "monokai",
        "https://raw.githubusercontent.com/microsoft/vscode/main/extensions/theme-monokai/themes/monokai-color-theme.json",
    ),
    ("Dracula", "dracula", ("dracula-theme/theme-dracula", "extension/theme/dracula.json")),
    ("One Dark Pro", "one-dark-pro", ("zhuangtongfa/material-theme", "extension/themes/OneDark-Pro.json")),
    ("Tokyo Night", "tokyo-night", ("enkia/tokyo-night", "extension/themes/tokyo-night-color-theme.json")),
    ("Nord", "nord", ("arcticicestudio/nord-visual-studio-code", "extension/themes/nord-color-theme.json")),
]


def read(source):
    if isinstance(source, str):
        with urllib.request.urlopen(source) as r:
            return r.read().decode(), source
    extension, member = source
    with urllib.request.urlopen(OPEN_VSX + extension + "/latest") as r:
        download = json.load(r)["files"]["download"]
    with urllib.request.urlopen(download) as r:
        package = zipfile.ZipFile(io.BytesIO(r.read()))
    return package.read(member).decode(), download


def plain_json(text):
    """JSONC to JSON: comments outside strings go, then trailing commas."""
    out, i, in_string = [], 0, False
    while i < len(text):
        c = text[i]
        if in_string and c == "\\":
            out.append(text[i : i + 2])
            i += 2
            continue
        if c == '"':
            in_string = not in_string
        elif not in_string and text.startswith("//", i):
            i = text.find("\n", i)
            if i < 0:
                break
            continue
        elif not in_string and text.startswith("/*", i):
            i = text.find("*/", i) + 2
            continue
        out.append(c)
        i += 1
    return re.sub(r",(\s*[}\]])", r"\1", "".join(out))


def fetch(source):
    text, url = read(source)
    theme = json.loads(plain_json(text))
    # A theme built on another: the parent's colours under the child's, the
    # parent's token rules before the child's so the child's win.
    if "include" in theme:
        parent = fetch(url.rsplit("/", 1)[0] + "/" + os.path.basename(theme.pop("include")))
        theme["colors"] = {**parent.get("colors", {}), **theme.get("colors", {})}
        theme["tokenColors"] = parent.get("tokenColors", []) + theme.get("tokenColors", [])
        theme.setdefault("type", parent.get("type"))
    theme["url"] = url
    return theme


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, slug, source in THEMES:
        theme = fetch(source)
        url = theme["url"]
        out = {
            "name": name,
            "type": theme.get("type", "dark"),
            "colors": theme.get("colors", {}),
            "tokenColors": theme.get("tokenColors", []),
        }
        path = os.path.join(OUT, slug + ".json")
        with open(path, "w") as f:
            f.write(f"// {name} (MIT), from {url}\n// as packaging/themes.py flattens it.\n")
            json.dump(out, f, indent=2, ensure_ascii=False)
            f.write("\n")
        print(f"{name:24} -> {os.path.relpath(path, ROOT)}")


if __name__ == "__main__":
    main()

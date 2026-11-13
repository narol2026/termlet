#!/usr/bin/env python3
"""Regenerate THIRD_PARTY_LICENSES.md from the Cargo dependency graph.

For every crate in the resolved dependency graph this walks the crate source
that Cargo already downloaded into the local registry, collects the license
files the crate ships, and emits a document with two parts: a table naming
every dependency and its declared license, and the full text of each distinct
license file (deduplicated) together with the packages that ship it. License
texts are reproduced verbatim so per-work copyright notices are preserved.

Usage:

    python3 tools/licenses/gen_third_party_licenses.py

Requires a checkout whose dependencies have been fetched (`cargo fetch`) and
Python 3.8+. Output is written to THIRD_PARTY_LICENSES.md next to this script's
repository root.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
OUTPUT = os.path.join(REPO_ROOT, "THIRD_PARTY_LICENSES.md")
ROOT_PACKAGE = "termlet"

LICENSE_PREFIXES = ("license", "licence", "copying", "unlicense", "notice")
MIN_TEXT_LEN = 40


def cargo_metadata() -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(out.stdout)


def license_files(pkg: dict):
    """Yield paths to the license files a crate ships, if any."""
    manifest = pkg.get("manifest_path")
    if not manifest:
        return
    crate_dir = os.path.dirname(manifest)
    if not os.path.isdir(crate_dir):
        return
    for entry in sorted(os.listdir(crate_dir)):
        path = os.path.join(crate_dir, entry)
        if not os.path.isfile(path):
            continue
        if entry.lower().startswith(LICENSE_PREFIXES):
            yield path


def fingerprint(text: str) -> str:
    normalized = re.sub(r"\s+", " ", text).strip().lower()
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest()[:16]


def collect(metadata: dict):
    packages = [p for p in metadata["packages"] if p["name"] != ROOT_PACKAGE]
    packages.sort(key=lambda p: (p["name"], p.get("version", "")))

    texts: dict[str, dict] = {}
    no_text: list[dict] = []

    for pkg in packages:
        found = False
        for path in license_files(pkg) or ():
            try:
                with open(path, encoding="utf-8", errors="replace") as handle:
                    text = handle.read().strip()
            except OSError:
                continue
            if len(text) < MIN_TEXT_LEN:
                continue
            found = True
            key = fingerprint(text)
            entry = texts.setdefault(key, {"text": text, "packages": []})
            entry["packages"].append(pkg)
        if not found:
            no_text.append(pkg)

    return packages, texts, no_text


def short_source(pkg: dict) -> str:
    repo = pkg.get("repository")
    if repo:
        return repo
    source = pkg.get("source") or ""
    if source.startswith("registry+"):
        return "crates.io"
    if source.startswith("git+"):
        return source.split("#")[0]
    return "—"


def render(packages, texts, no_text) -> str:
    lines: list[str] = []
    add = lines.append

    add("# Third-party licenses")
    add("")
    add("Termlet is distributed under Apache-2.0 (see [LICENSE](LICENSE)). It")
    add("builds on third-party open-source software, embeds two fonts, and builds")
    add("dependencies from five patched forks. This file lists every component and")
    add("reproduces the license texts required for redistribution.")
    add("")
    add("It is generated from the resolved Cargo dependency graph by")
    add("`tools/licenses/gen_third_party_licenses.py`. Regenerate it after dependency")
    add("changes with:")
    add("")
    add("```bash")
    add("cargo fetch")
    add("python3 tools/licenses/gen_third_party_licenses.py")
    add("```")
    add("")
    add("## Bundled components")
    add("")
    add("These are redistributed directly in this repository or compiled into the")
    add("binary, beyond the ordinary crate dependencies listed below.")
    add("")
    add("| Component | Version | License | Notes |")
    add("| --- | --- | --- | --- |")
    add("| Source Code Pro | 2.042 | SIL OFL-1.1 | Embedded terminal font. Full text in"
        " `fonts/licenses/SourceCodePro-OFL-1.1.txt`. |")
    add("| Lucide | — | ISC | Icon glyphs embedded via the `lucide-icons` crate."
        " Notice in `fonts/licenses/Lucide-ISC.txt`. |")
    add("| Font Awesome Free | 7.2.0 | CC BY 4.0 (icons) | Platform/OS glyphs in"
        " `assets/icon-font/` (debian, ubuntu, centos, linux, windows, macos), compiled"
        " into the `my-icons` font. See <https://fontawesome.com/license/free>. |")
    add("")
    add("The patched `iced` family, `alacritty_terminal`, `vte`, `iced_term`, and `iced_drop`")
    add("crates are built from forks pinned to exact commits in the")
    add("[`Cargo.toml` patch table](Cargo.toml); they are listed below in the")
    add("dependency table under their fork sources.")
    add("")
    add("The `my-icons` font under `fonts/icons/` is generated from `assets/icon-font/`,")
    add("which mixes Font Awesome Free glyphs (see above) with project-authored")
    add("ones. The application UI bundles no typeface and renders with the platform")
    add("default sans-serif.")
    add("")
    add("## Dependencies")
    add("")
    add(f"The resolved dependency graph contains {len(packages)} crates.")
    add("")
    add("| Package | Version | License | Source |")
    add("| --- | --- | --- | --- |")
    for pkg in packages:
        license = pkg.get("license") or "see package"
        add(f"| {pkg['name']} | {pkg.get('version', '')} | {license} | {short_source(pkg)} |")
    add("")

    if no_text:
        add("The following crates declare a license but ship no license file in")
        add("their published source; refer to the crate's repository for the text:")
        add("")
        for pkg in no_text:
            add(f"- {pkg['name']} {pkg.get('version', '')} — {pkg.get('license') or 'see package'}"
                f" ({short_source(pkg)})")
        add("")

    add("## License texts")
    add("")
    add("Each distinct license file is reproduced once, followed by the packages")
    add("that ship it.")
    add("")
    for entry in sorted(texts.values(), key=lambda e: min(p["name"] for p in e["packages"])):
        names = sorted({f"{p['name']} {p.get('version', '')}" for p in entry["packages"]})
        add(f"### {names[0]}" + (f" and {len(names) - 1} more" if len(names) > 1 else ""))
        add("")
        add("Applies to: " + ", ".join(names))
        add("")
        # Pick a fence longer than any backtick run inside the text (some
        # licenses contain Markdown code samples) so the block never closes
        # early.
        longest_run = max((len(run) for run in re.findall(r"`+", entry["text"])), default=0)
        fence = "`" * max(3, longest_run + 1)
        add(fence)
        add(entry["text"].rstrip())
        add(fence)
        add("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    metadata = cargo_metadata()
    packages, texts, no_text = collect(metadata)
    document = render(packages, texts, no_text)
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write(document)
    print(
        f"wrote {OUTPUT} — {len(packages)} crates, "
        f"{len(texts)} distinct license texts, {len(no_text)} without a text",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

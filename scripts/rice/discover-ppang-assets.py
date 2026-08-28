#!/usr/bin/env python3
"""Discover the official PPanG chromosome 6 Minigraph-Cactus XG URL."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import subprocess
import sys
import urllib.parse
import urllib.request
from html.parser import HTMLParser


PAGE_URL = "https://cgm.sjtu.edu.cn/PPanG/"
TARGET = "chr06_mc.xg"
OFFICIAL_HOST = "cgm.sjtu.edu.cn"


class AnchorParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.anchors: list[dict[str, str]] = []
        self._href: str | None = None
        self._text: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() != "a":
            return
        self._href = dict(attrs).get("href")
        self._text = []

    def handle_data(self, data: str) -> None:
        if self._href is not None:
            self._text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "a" and self._href is not None:
            self.anchors.append({"href": self._href, "text": "".join(self._text).strip()})
            self._href = None
            self._text = []


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def parse_anchors(html: str, base_url: str) -> list[dict[str, str]]:
    parser = AnchorParser()
    parser.feed(html)
    return [
        {
            "text": anchor["text"],
            "url": urllib.parse.urljoin(base_url, anchor["href"]),
        }
        for anchor in parser.anchors
    ]


def matching_urls(anchors: list[dict[str, str]]) -> list[str]:
    matches = {
        anchor["url"]
        for anchor in anchors
        if anchor["text"] == TARGET
        or pathlib.PurePosixPath(urllib.parse.urlparse(anchor["url"]).path).name == TARGET
    }
    return sorted(matches)


def validate_url(url: str, explicitly_linked_hosts: set[str]) -> None:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https":
        raise RuntimeError(f"discovered URL is not HTTPS: {url}")
    if not parsed.hostname:
        raise RuntimeError(f"discovered URL has no host: {url}")
    if parsed.hostname != OFFICIAL_HOST and parsed.hostname not in explicitly_linked_hosts:
        raise RuntimeError(
            f"discovered host {parsed.hostname!r} is neither the official PPanG host "
            "nor a host explicitly linked by the PPanG page"
        )


PLAYWRIGHT_SCRIPT = r"""
import fs from "node:fs/promises";
import path from "node:path";
import { chromium } from "@playwright/test";

const pageUrl = process.env.PPANG_PAGE_URL;
const target = process.env.PPANG_TARGET;
const output = process.env.PPANG_PLAYWRIGHT_OUTPUT;
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  const responses = [];
  page.on("response", (response) => {
    responses.push({ status: response.status(), url: response.url() });
  });
  await page.goto(pageUrl, { waitUntil: "networkidle", timeout: 120000 });
  const anchors = await page.locator("a").evaluateAll((links) =>
    links.map((link) => ({
      text: (link.textContent ?? "").trim(),
      url: link.href,
    })),
  );
  const matches = anchors.filter((anchor) => {
    if (!anchor.url) return anchor.text === target;
    try {
      const basename = new URL(anchor.url).pathname.split("/").filter(Boolean).at(-1);
      return anchor.text === target || basename === target;
    } catch {
      return anchor.text === target;
    }
  });
  let probe = null;
  if (matches.length > 0) {
    const response = await page.request.fetch(matches[0].url, {
      method: "GET",
      headers: { Range: "bytes=0-0" },
      failOnStatusCode: false,
      timeout: 120000,
    });
    probe = {
      requestedUrl: matches[0].url,
      finalUrl: response.url(),
      status: response.status(),
      headers: response.headers(),
    };
    await response.dispose();
  }
  const renderedHtmlPath = path.join(path.dirname(output), "ppang-rendered.html");
  await fs.writeFile(renderedHtmlPath, await page.content());
  await fs.writeFile(output, JSON.stringify({ anchors, matches, probe, responses }, null, 2) + "\n");
} finally {
  await browser.close();
}
"""


def playwright_discover(output_dir: pathlib.Path) -> tuple[list[dict[str, str]], dict[str, object]]:
    log_path = output_dir / "ppang-playwright.json"
    env = os.environ.copy()
    env.update(
        {
            "PPANG_PAGE_URL": PAGE_URL,
            "PPANG_TARGET": TARGET,
            "PPANG_PLAYWRIGHT_OUTPUT": str(log_path.resolve()),
        }
    )
    command = [
        "pnpm",
        "--filter",
        "@pangenome-range/benchmark",
        "exec",
        "node",
        "--input-type=module",
        "--eval",
        PLAYWRIGHT_SCRIPT,
    ]
    try:
        subprocess.run(command, check=True, env=env)
    except FileNotFoundError as error:
        raise RuntimeError("Playwright fallback requires pnpm, Node.js, and workspace dependencies") from error
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"Playwright fallback failed with exit status {error.returncode}") from error
    data = json.loads(log_path.read_text())
    return data["anchors"], data


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=pathlib.Path,
        default=pathlib.Path("data/rice/provenance"),
        help="directory for original HTML and discovery logs",
    )
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    discovered_at = utc_now()
    request = urllib.request.Request(PAGE_URL, headers={"User-Agent": "pangenome-range-rice-acquisition/1"})
    with urllib.request.urlopen(request, timeout=120) as response:
        final_page_url = response.geturl()
        html_bytes = response.read()
        status = response.status
        headers = dict(response.headers.items())

    html_path = args.output_dir / "ppang-original.html"
    html_path.write_bytes(html_bytes)
    html = html_bytes.decode("utf-8", errors="replace")
    anchors = parse_anchors(html, final_page_url)
    matches = matching_urls(anchors)
    method = "static-html"
    browser_log: dict[str, object] | None = None

    if not matches:
        anchors, browser_log = playwright_discover(args.output_dir)
        matches = matching_urls(anchors)
        method = "playwright"

    if not matches:
        raise RuntimeError(f"no anchor matching {TARGET!r} was found")
    if len(matches) != 1:
        raise RuntimeError(f"found more than one different URL matching {TARGET!r}: {matches}")

    linked_hosts = {
        host
        for anchor in anchors
        if (host := urllib.parse.urlparse(anchor["url"]).hostname) is not None
    }
    discovered_url = matches[0]
    final_url = discovered_url
    if browser_log and isinstance(browser_log.get("probe"), dict):
        final_url = str(browser_log["probe"].get("finalUrl") or discovered_url)
    validate_url(discovered_url, linked_hosts)
    validate_url(final_url, linked_hosts)

    record = {
        "schemaVersion": 1,
        "discoveredAt": discovered_at,
        "pageUrl": PAGE_URL,
        "finalPageUrl": final_page_url,
        "pageStatus": status,
        "pageHeaders": headers,
        "target": TARGET,
        "method": method,
        "discoveredUrl": discovered_url,
        "finalUrl": final_url,
        "originalHtml": str(html_path),
        "playwrightLog": str(args.output_dir / "ppang-playwright.json") if browser_log else None,
    }
    (args.output_dir / "ppang-discovery.json").write_text(json.dumps(record, indent=2) + "\n")
    print(final_url)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)

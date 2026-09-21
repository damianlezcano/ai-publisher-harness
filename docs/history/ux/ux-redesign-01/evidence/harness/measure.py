import asyncio
import json
import os
import sys
from pathlib import Path
from playwright.async_api import async_playwright

BASE = "http://localhost:1420/"
HERE = Path(__file__).resolve().parent
EVIDENCE = HERE.parent
MOCK_SRC = (HERE / "mock-inject.js").read_text(encoding="utf-8")

VIEWPORTS = {
    "1366x768": (1366, 768),
    "1440x900": (1440, 900),
    "1920x1080": (1920, 1080),
}

MEASURE_JS = """
() => {
  const out = {};
  out.viewport = { w: window.innerWidth, h: window.innerHeight };
  out.doc = {
    scrollW: document.documentElement.scrollWidth,
    scrollH: document.documentElement.scrollHeight,
    bodyScrollW: document.body.scrollWidth,
  };
  const box = (sel, label) => {
    const el = document.querySelector(sel);
    if (!el) { out[label] = null; return; }
    const r = el.getBoundingClientRect();
    out[label] = {
      x: Math.round(r.x), y: Math.round(r.y),
      w: Math.round(r.width), h: Math.round(r.height),
      bottom: Math.round(r.bottom), right: Math.round(r.right),
    };
  };
  box('.conversations-sidebar', 'sidebar');
  box('.composer-bar', 'composerBar');
  box('.composer-model-select', 'composerModelSelect');
  box('.share-control', 'shareControl');
  out.hasWorkspaceGrid = !!document.querySelector('.workspace-grid');
  out.headerModelSelector = !!document.querySelector('.app-shell-header .model-selector, header .model-selector');
  out.hasHorizontalOverflow = document.documentElement.scrollWidth > (window.innerWidth + 1);
  return out;
}
"""


def check(data, vp):
    issues = []
    if data["hasWorkspaceGrid"]:
        issues.append("workspace-grid still present")
    if data["headerModelSelector"]:
        issues.append("model-selector still in header")
    if not data["sidebar"]:
        issues.append("conversations-sidebar missing")
    if not data["composerBar"]:
        issues.append("composer-bar missing")
    elif data["composerBar"]["bottom"] > data["viewport"]["h"] + 1:
        issues.append("composer-bar bottom edge below viewport")
    if not data["composerModelSelect"]:
        issues.append("composer-model-select missing")
    if not data["shareControl"]:
        issues.append("share-control missing")
    if data["hasHorizontalOverflow"]:
        issues.append("horizontal overflow detected")
    return issues


async def measure(browser, seed, vp):
    ctx = await browser.new_context(
        viewport={"width": VIEWPORTS[vp][0], "height": VIEWPORTS[vp][1]},
        permissions=["clipboard-read", "clipboard-write"],
    )
    await ctx.add_init_script("window.__UX_SEED__ = " + json.dumps(seed))
    await ctx.add_init_script(MOCK_SRC)
    page = await ctx.new_page()
    await page.goto(BASE)
    await page.wait_for_selector(".app-shell", timeout=15000)
    await page.wait_for_timeout(500)
    data = await page.evaluate(MEASURE_JS)
    issues = check(data, vp)
    print(f"--- {seed} [{vp}] viewport={data['viewport']} doc={data['doc']}")
    print("  sidebar:", data["sidebar"])
    print("  composerBar:", data["composerBar"])
    print("  composerModelSelect:", data["composerModelSelect"])
    print("  shareControl:", data["shareControl"])
    print("  hasWorkspaceGrid:", data["hasWorkspaceGrid"])
    print("  headerModelSelector:", data["headerModelSelector"])
    print("  hasHorizontalOverflow:", data["hasHorizontalOverflow"])
    if issues:
        print("  ISSUES:", "; ".join(issues))
    await ctx.close()
    return data, issues


async def main():
    headless = os.environ.get("PLAYWRIGHT_HEADLESS", "0") == "1"
    all_issues = []
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=headless)
        try:
            for vp in VIEWPORTS:
                _, issues = await measure(browser, "workspace", vp)
                all_issues.extend(issues)
        finally:
            await browser.close()
    if all_issues:
        print("\nMEASURE FAILED:", "; ".join(all_issues))
        sys.exit(1)
    print("\nMEASURE PASSED")


if __name__ == "__main__":
    asyncio.run(main())

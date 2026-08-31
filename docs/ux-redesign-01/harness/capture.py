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
DEFAULT_VP = "1440x900"


def shot(name):
    return EVIDENCE / (name + ".png")


def a11y_path(name):
    return EVIDENCE / (name + ".a11y.txt")


async def new_page(browser, seed, vp=DEFAULT_VP):
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
    return ctx, page


async def a11y_tree(page, name):
    try:
        cdp = await page.context.new_cdp_session(page)
        res = await cdp.send("Accessibility.getFullAXTree")
        lines = []
        for node in res.get("nodes", []):
            n = node.get("name") or ""
            role = node.get("role", {}).get("value", "")
            if n and role not in ("none", "generic", "InlineTextBox", "StaticText"):
                lines.append(f"[{role}] {n}")
        a11y_path(name).write_text("\n".join(lines), encoding="utf-8")
        print("  a11y tree saved:", name)
    except Exception as e:
        print("  a11y skip:", e)


async def screenshot(page, name, full_page=True):
    await page.screenshot(path=str(shot(name)), full_page=full_page)
    print("  saved", name)


async def get_header_text(page):
    return await page.locator(".workspace-header h1").text_content()


class AssertionLog:
    def __init__(self):
        self.passed = []
        self.failed = []

    def check(self, name, condition, evidence):
        if condition:
            self.passed.append((name, evidence))
            print(f"  PASS: {name}")
        else:
            self.failed.append((name, evidence))
            print(f"  FAIL: {name} ({evidence})")

    def raise_if_failed(self):
        if self.failed:
            raise AssertionError(f"{len(self.failed)} assertion(s) failed")


LOG = AssertionLog()


async def run(browser):
    # 1. First launch (3 viewports) -----------------------------------------
    print("== 01 first-launch ==")
    for vp in VIEWPORTS:
        ctx, page = await new_page(browser, "first-launch", vp)
        await screenshot(page, f"01-first-launch-{vp}")
        if vp == DEFAULT_VP:
            await a11y_tree(page, "01-first-launch")
            a11y = a11y_path("01-first-launch").read_text(encoding="utf-8")
            LOG.check("sidebar title is Conversaciones", "Conversaciones" in a11y, "01-first-launch.a11y.txt")
            LOG.check("free Gratis badge present", "Gratis" in a11y, "01-first-launch.a11y.txt")
            LOG.check("no raw :: id text", "::" not in a11y, "01-first-launch.a11y.txt")
            LOG.check("no workspace-grid", await page.locator(".workspace-grid").count() == 0, "01-first-launch.png")
            LOG.check("no header model-selector", await page.locator(".app-shell-header .model-selector").count() == 0, "01-first-launch.png")
            LOG.check("no Mis proyectos text", "Mis proyectos" not in await page.content(), "01-first-launch.png")
        await ctx.close()

    # 2. Conversation list (3 viewports) ------------------------------------
    print("== 02 conversation-list ==")
    for vp in VIEWPORTS:
        ctx, page = await new_page(browser, "list", vp)
        await screenshot(page, f"02-conversation-list-{vp}")
        if vp == DEFAULT_VP:
            await a11y_tree(page, "02-conversation-list")
            names = await page.locator(".conversation-select .conversation-name").all_text_contents()
            LOG.check("newest-first order", names == ["Fracciones", "Sistema solar", "Fotosíntesis"], f"order={names}")
            timestamps = await page.locator(".conversation-timestamp").count()
            LOG.check("timestamps present", timestamps == 3, f"timestamps={timestamps}")
            shared = await page.locator(".conversation-shared-badge").count()
            LOG.check("shared badge present", shared >= 1, f"shared_badges={shared}")
        await ctx.close()

    # 3. Rename --------------------------------------------------------------
    print("== 03 rename ==")
    ctx, page = await new_page(browser, "list")
    row = page.locator(".conversation-item", has_text="Sistema solar")
    await row.locator(".conversation-rename-button").click()
    await page.wait_for_timeout(300)
    await page.fill('input[id^="rename-"]', "El sistema solar")
    await page.get_by_role("button", name="Guardar").click()
    await page.wait_for_timeout(500)
    await screenshot(page, "03-rename-saved")
    await a11y_tree(page, "03-rename-saved")
    names = await page.locator(".conversation-select .conversation-name").all_text_contents()
    LOG.check("rename saved", "El sistema solar" in names, f"names={names}")
    await ctx.close()

    # 3b. Rename cancel with Esc
    ctx, page = await new_page(browser, "list")
    row = page.locator(".conversation-item", has_text="Sistema solar")
    await row.locator(".conversation-rename-button").click()
    await page.wait_for_timeout(300)
    await page.fill('input[id^="rename-"]', "Cancelado")
    await page.keyboard.press("Escape")
    await page.wait_for_timeout(400)
    await screenshot(page, "03-rename-cancelled")
    names = await page.locator(".conversation-select .conversation-name").all_text_contents()
    LOG.check("rename Esc cancelled", "Cancelado" not in names, f"names={names}")
    await ctx.close()

    # 4. Send prompt (working + completed) ----------------------------------
    print("== 04 send ==")
    ctx, page = await new_page(browser, "workspace")
    prompt = "Creá una actividad interactiva sobre la fotosíntesis"
    await page.fill("#composer-prompt", prompt)
    await page.get_by_role("button", name="Enviar").click()
    await page.wait_for_timeout(300)
    await screenshot(page, "04-send-working")
    await a11y_tree(page, "04-send-working")
    working_count = await page.locator(".message").count()
    chat_status = await page.locator(".chat-status").text_content()
    LOG.check("working state shows creating", "Creando" in (chat_status or ""), f"status={chat_status}")
    LOG.check("user message bubble appears", prompt in await page.locator(".message-user .message-text").last.text_content(), "04-send-working.png")
    await page.wait_for_timeout(1100)
    await screenshot(page, "04-send-completed")
    await a11y_tree(page, "04-send-completed")
    completed_count = await page.locator(".message").count()
    creations = await page.locator(".creation-card").count()
    LOG.check("assistant message added", completed_count > working_count, f"working={working_count} completed={completed_count}")
    LOG.check("inline creation card appears", creations >= 1, f"creations={creations}")
    await ctx.close()

    # 5. Resources -----------------------------------------------------------
    print("== 05 resources ==")
    ctx, page = await new_page(browser, "workspace")
    await screenshot(page, "05-resources")
    await a11y_tree(page, "05-resources")
    user_chips = await page.locator(".message-user .chip").all_text_contents()
    LOG.check("user message material chips", "manual.pdf" in user_chips and "esquema-fotosíntesis.png" in user_chips, f"chips={user_chips}")
    assistant_cards = await page.locator(".message-assistant .creation-card").count()
    LOG.check("assistant inline creation cards", assistant_cards >= 1, f"cards={assistant_cards}")
    unattached = await page.locator(".workspace-materials .material-list li").all_text_contents()
    LOG.check("unattached Materiales lists only unreferenced", "diapo.pptx" in " ".join(unattached), f"unattached={unattached}")
    LOG.check("manual.pdf not duplicated in unattached", "manual.pdf" not in " ".join(unattached), "05-resources.png")
    await ctx.close()

    # 6. Settings (open + close returns to same conversation) ---------------
    print("== 06 settings ==")
    ctx, page = await new_page(browser, "workspace")
    before_title = await get_header_text(page)
    await page.locator(".app-settings-button").click()
    await page.wait_for_selector(".provider-dialog", timeout=10000)
    await page.wait_for_timeout(400)
    await screenshot(page, "06-settings-open")
    await a11y_tree(page, "06-settings-open")
    dialog_title = await page.locator(".provider-dialog h2").text_content()
    LOG.check("settings dialog titled Configuración", dialog_title == "Configuración", f"title={dialog_title}")
    await page.locator(".provider-dialog .close-button").click()
    await page.wait_for_timeout(500)
    after_title = await get_header_text(page)
    await screenshot(page, "06-settings-closed")
    await a11y_tree(page, "06-settings-closed")
    LOG.check("close X returns to same conversation", before_title == after_title, f"before={before_title} after={after_title}")
    await ctx.close()

    # 7. Share (busy + shared menu) -----------------------------------------
    print("== 07 share ==")
    ctx, page = await new_page(browser, "workspace")
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await screenshot(page, "07-share-busy")
    busy_text = await page.locator(".share-control-trigger").text_content()
    LOG.check("share busy state", "Compartiendo" in busy_text, f"text={busy_text}")
    await page.wait_for_timeout(1200)
    await screenshot(page, "07-share-shared")
    shared_text = await page.locator(".share-control-trigger").text_content()
    LOG.check("share shared state", "Compartido" in shared_text, f"text={shared_text}")
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await screenshot(page, "07-share-menu")
    await a11y_tree(page, "07-share-menu")
    menu_items = await page.locator(".share-control-menu button").all_text_contents()
    LOG.check("menu has Copiar enlace", "Copiar enlace" in menu_items, f"items={menu_items}")
    LOG.check("menu has Abrir enlace", "Abrir enlace" in menu_items, f"items={menu_items}")
    LOG.check("menu has Mostrar QR", "Mostrar QR" in menu_items, f"items={menu_items}")
    LOG.check("menu has Dejar de compartir", "Dejar de compartir" in menu_items, f"items={menu_items}")
    await ctx.close()

    # 8. QR dialog -----------------------------------------------------------
    print("== 08 qr ==")
    ctx, page = await new_page(browser, "shared")
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await page.get_by_role("menuitem", name="Mostrar QR").click()
    await page.wait_for_selector(".qr", timeout=10000)
    await page.wait_for_timeout(400)
    await screenshot(page, "08-qr")
    await a11y_tree(page, "08-qr")
    LOG.check("QR dialog visible", await page.locator(".qr").count() == 1, "08-qr.png")
    await ctx.close()

    # 9. Stop sharing --------------------------------------------------------
    print("== 09 stop-sharing ==")
    ctx, page = await new_page(browser, "shared")
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await page.get_by_role("menuitem", name="Dejar de compartir").click()
    await page.wait_for_selector(".dialog-backdrop", timeout=10000)
    await page.wait_for_timeout(300)
    await screenshot(page, "09-stop-confirm")
    await a11y_tree(page, "09-stop-confirm")
    confirm_title = await page.locator(".dialog h2").text_content()
    LOG.check("stop confirmation title", "Dejar de compartir" in confirm_title, f"title={confirm_title}")
    await page.get_by_role("button", name="Confirmar").click()
    await page.wait_for_timeout(800)
    await screenshot(page, "09-stop-stopped")
    await a11y_tree(page, "09-stop-stopped")
    stopped_text = await page.locator(".share-control-trigger").text_content()
    LOG.check("back to Compartir", "Compartir" in stopped_text and "Compartido" not in stopped_text, f"text={stopped_text}")
    await ctx.close()

    # 10. Restart persistence ------------------------------------------------
    print("== 10 restart ==")
    ctx, page = await new_page(browser, "workspace")
    await page.fill("#composer-prompt", "Generá una guía de estudio")
    await page.get_by_role("button", name="Enviar").click()
    await page.wait_for_timeout(1500)
    before_count = await page.locator(".message").count()
    before_title = await get_header_text(page)
    await screenshot(page, "10-restart-before")
    await page.reload()
    await page.wait_for_selector(".app-shell", timeout=15000)
    await page.wait_for_timeout(800)
    after_count = await page.locator(".message").count()
    after_title = await get_header_text(page)
    await screenshot(page, "10-restart-after")
    await a11y_tree(page, "10-restart-after")
    LOG.check("messages restored after reload", after_count == before_count and after_count >= 2, f"before={before_count} after={after_count}")
    LOG.check("conversation title restored", before_title == after_title, f"before={before_title} after={after_title}")
    await ctx.close()


async def main():
    headless = os.environ.get("PLAYWRIGHT_HEADLESS", "0") == "1"
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=headless)
        try:
            await run(browser)
            LOG.raise_if_failed()
            print(f"\nCAPTURE PASSED ({len(LOG.passed)} assertions)")
        finally:
            await browser.close()


if __name__ == "__main__":
    asyncio.run(main())

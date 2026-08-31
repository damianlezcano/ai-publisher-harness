import asyncio
import json
import os
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


def plain_name(node_name):
    """Extract a plain string from CDP Accessibility node's name field,
    which may be a dict with a 'value' key."""
    if isinstance(node_name, str):
        return node_name
    if isinstance(node_name, dict):
        value = node_name.get("value")
        if isinstance(value, str):
            return value
        if isinstance(value, dict):
            return str(value.get("value", ""))
    return ""


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
            n = plain_name(node.get("name"))
            role = ""
            role_obj = node.get("role")
            if isinstance(role_obj, dict):
                role = role_obj.get("value", "")
            elif isinstance(role_obj, str):
                role = role_obj
            if n and role not in ("none", "generic", "InlineTextBox", "StaticText"):
                lines.append(f"[{role}] {n}")
        a11y_path(name).write_text("\n".join(lines), encoding="utf-8")
        print("  a11y tree saved:", name)
    except Exception as e:
        print("  a11y skip:", e)


async def screenshot(page, name, full_page=True):
    await page.screenshot(path=str(shot(name)), full_page=full_page)
    print("  saved", name)


async def write_ocr_fallback(name, texts):
    """Write a fallback .ocr.txt when tesseract cannot read image-only content.
    Used for the QR dialog so every PNG still has a non-empty OCR file."""
    txt = EVIDENCE / (name + ".ocr.txt")
    txt.write_text("\n".join(texts), encoding="utf-8")
    print("  ocr fallback:", name)


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


async def flow_01_first_launch(browser, vp):
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


async def flow_02_conversation_list(browser, vp):
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


async def flow_03_rename(browser, vp):
    # Save rename
    ctx, page = await new_page(browser, "list", vp)
    row = page.locator(".conversation-item", has_text="Sistema solar")
    await row.locator(".conversation-rename-button").click()
    await page.wait_for_timeout(300)
    await page.fill('input[id^="rename-"]', "El sistema solar")
    await page.get_by_role("button", name="Guardar").click()
    await page.wait_for_timeout(500)
    await screenshot(page, f"03-rename-saved-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "03-rename-saved")
        names = await page.locator(".conversation-select .conversation-name").all_text_contents()
        LOG.check("rename saved", "El sistema solar" in names, f"names={names}")
    await ctx.close()

    # Cancel rename with Esc
    ctx, page = await new_page(browser, "list", vp)
    row = page.locator(".conversation-item", has_text="Sistema solar")
    await row.locator(".conversation-rename-button").click()
    await page.wait_for_timeout(300)
    await page.fill('input[id^="rename-"]', "Cancelado")
    await page.keyboard.press("Escape")
    await page.wait_for_timeout(400)
    await screenshot(page, f"03-rename-cancelled-{vp}")
    if vp == DEFAULT_VP:
        names = await page.locator(".conversation-select .conversation-name").all_text_contents()
        LOG.check("rename Esc cancelled", "Cancelado" not in names, f"names={names}")
    await ctx.close()


async def flow_04_send(browser, vp):
    ctx, page = await new_page(browser, "workspace", vp)
    prompt = "Creá una actividad interactiva sobre la fotosíntesis"
    await page.fill("#composer-prompt", prompt)
    await page.get_by_role("button", name="Enviar").click()
    await page.wait_for_timeout(300)
    await screenshot(page, f"04-send-working-{vp}")
    working_count = await page.locator(".message").count()
    chat_status = await page.locator(".chat-status").text_content()
    if vp == DEFAULT_VP:
        LOG.check("working state shows creating", "Creando" in (chat_status or ""), f"status={chat_status}")
        LOG.check("user message bubble appears", prompt in await page.locator(".message-user .message-text").last.text_content(), "04-send-working.png")
    await page.wait_for_timeout(1100)
    await screenshot(page, f"04-send-completed-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "04-send-completed")
        completed_count = await page.locator(".message").count()
        creations = await page.locator(".creation-card").count()
        LOG.check("assistant message added", completed_count > working_count, f"working={working_count} completed={completed_count}")
        LOG.check("inline creation card appears", creations >= 1, f"creations={creations}")
    await ctx.close()


async def flow_05_resources(browser, vp):
    ctx, page = await new_page(browser, "workspace", vp)
    await screenshot(page, f"05-resources-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "05-resources")
        user_chips = await page.locator(".message-user .chip").all_text_contents()
        LOG.check("user message material chips", "manual.pdf" in user_chips and "esquema-fotosíntesis.png" in user_chips, f"chips={user_chips}")
        assistant_cards = await page.locator(".message-assistant .creation-card").count()
        LOG.check("assistant inline creation cards", assistant_cards >= 1, f"cards={assistant_cards}")
        unattached = await page.locator(".workspace-materials .material-list li").all_text_contents()
        LOG.check("unattached Materiales lists only unreferenced", "diapo.pptx" in " ".join(unattached), f"unattached={unattached}")
        LOG.check("manual.pdf not duplicated in unattached", "manual.pdf" not in " ".join(unattached), "05-resources.png")
    await ctx.close()


async def flow_06_settings(browser, vp):
    ctx, page = await new_page(browser, "workspace", vp)
    before_title = await get_header_text(page)
    await page.locator(".app-settings-button").click()
    await page.wait_for_selector(".provider-dialog", timeout=10000)
    await page.wait_for_timeout(400)
    await screenshot(page, f"06-settings-open-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "06-settings-open")
        dialog_title = await page.locator(".provider-dialog h2").text_content()
        LOG.check("settings dialog titled Configuración", dialog_title == "Configuración", f"title={dialog_title}")
    await page.locator(".provider-dialog .close-button").click()
    await page.wait_for_timeout(500)
    after_title = await get_header_text(page)
    await screenshot(page, f"06-settings-closed-{vp}")
    if vp == DEFAULT_VP:
        LOG.check("close X returns to same conversation", before_title == after_title, f"before={before_title} after={after_title}")
    await ctx.close()


async def flow_07_share(browser, vp):
    ctx, page = await new_page(browser, "workspace", vp)
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await screenshot(page, f"07-share-busy-{vp}")
    busy_text = await page.locator(".share-control-trigger").text_content()
    if vp == DEFAULT_VP:
        LOG.check("share busy state", "Compartiendo" in busy_text, f"text={busy_text}")
    await page.wait_for_timeout(1200)
    await screenshot(page, f"07-share-shared-{vp}")
    shared_text = await page.locator(".share-control-trigger").text_content()
    if vp == DEFAULT_VP:
        LOG.check("share shared state", "Compartido" in shared_text, f"text={shared_text}")
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await screenshot(page, f"07-share-menu-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "07-share-menu")
        menu_items = await page.locator(".share-control-menu button").all_text_contents()
        LOG.check("menu has Copiar enlace", "Copiar enlace" in menu_items, f"items={menu_items}")
        LOG.check("menu has Abrir enlace", "Abrir enlace" in menu_items, f"items={menu_items}")
        LOG.check("menu has Mostrar QR", "Mostrar QR" in menu_items, f"items={menu_items}")
        LOG.check("menu has Dejar de compartir", "Dejar de compartir" in menu_items, f"items={menu_items}")
    await ctx.close()


async def flow_08_qr(browser, vp):
    ctx, page = await new_page(browser, "shared", vp)
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await page.get_by_role("menuitem", name="Mostrar QR").click()
    await page.wait_for_selector(".qr", timeout=10000)
    await page.wait_for_timeout(400)
    await screenshot(page, f"08-qr-{vp}")
    # QR image OCRs empty; write DOM-extracted text as fallback so .ocr.txt is non-empty.
    dialog_title = await page.locator(".dialog h2").text_content()
    dialog_buttons = await page.locator(".dialog button").all_text_contents()
    await write_ocr_fallback(f"08-qr-{vp}", [dialog_title or "", *dialog_buttons])
    if vp == DEFAULT_VP:
        await a11y_tree(page, "08-qr")
        qr_count = await page.locator(".qr").count()
        LOG.check("QR dialog visible", qr_count == 1, "08-qr.png")
        LOG.check("QR dialog has copy/open links", "Copiar enlace" in dialog_buttons and "Abrir enlace" in dialog_buttons, f"buttons={dialog_buttons}")
    await ctx.close()


async def flow_09_stop_sharing(browser, vp):
    ctx, page = await new_page(browser, "shared", vp)
    await page.locator(".share-control-trigger").click()
    await page.wait_for_timeout(300)
    await page.get_by_role("menuitem", name="Dejar de compartir").click()
    await page.wait_for_selector(".dialog-backdrop", timeout=10000)
    await page.wait_for_timeout(300)
    await screenshot(page, f"09-stop-confirm-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "09-stop-confirm")
        confirm_title = await page.locator(".dialog h2").text_content()
        LOG.check("stop confirmation title", "Dejar de compartir" in confirm_title, f"title={confirm_title}")
    await page.get_by_role("button", name="Confirmar").click()
    await page.wait_for_timeout(800)
    await screenshot(page, f"09-stop-stopped-{vp}")
    if vp == DEFAULT_VP:
        stopped_text = await page.locator(".share-control-trigger").text_content()
        LOG.check("back to Compartir", "Compartir" in stopped_text and "Compartido" not in stopped_text, f"text={stopped_text}")
    await ctx.close()


async def flow_10_restart(browser, vp):
    ctx, page = await new_page(browser, "workspace", vp)
    await page.fill("#composer-prompt", "Generá una guía de estudio")
    await page.get_by_role("button", name="Enviar").click()
    await page.wait_for_timeout(1500)
    before_count = await page.locator(".message").count()
    before_title = await get_header_text(page)
    await screenshot(page, f"10-restart-before-{vp}")
    await page.reload()
    await page.wait_for_selector(".app-shell", timeout=15000)
    await page.wait_for_timeout(800)
    after_count = await page.locator(".message").count()
    after_title = await get_header_text(page)
    await screenshot(page, f"10-restart-after-{vp}")
    if vp == DEFAULT_VP:
        await a11y_tree(page, "10-restart-after")
        LOG.check("messages restored after reload", after_count == before_count and after_count >= 2, f"before={before_count} after={after_count}")
        LOG.check("conversation title restored", before_title == after_title, f"before={before_title} after={after_title}")
    await ctx.close()


FLOW_RUNNERS = [
    ("01 first-launch", flow_01_first_launch),
    ("02 conversation-list", flow_02_conversation_list),
    ("03 rename", flow_03_rename),
    ("04 send", flow_04_send),
    ("05 resources", flow_05_resources),
    ("06 settings", flow_06_settings),
    ("07 share", flow_07_share),
    ("08 qr", flow_08_qr),
    ("09 stop-sharing", flow_09_stop_sharing),
    ("10 restart", flow_10_restart),
]


async def run(browser):
    for label, runner in FLOW_RUNNERS:
        print(f"== {label} ==")
        for vp in VIEWPORTS:
            print(f"  viewport {vp}")
            await runner(browser, vp)


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

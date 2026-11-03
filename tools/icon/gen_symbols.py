#!/usr/bin/env python3
"""Generates the app's icons from Material Symbols (Apache 2.0).

PassIcons.kt holds one getter per glyph, and each style has its own table of paths, so the whole
app changes icon style by flipping one value. Names ending in `!` also get a filled twin, for the
selected state of a tab. Run from the repository root; downloaded SVGs are cached in target/symbols.
"""
import concurrent.futures, pathlib, re, urllib.request

# name in code -> Google's family, the variant for plain and for filled
STYLES = {
    "Outlined": ("materialsymbolsoutlined", "default", "fill1"),
    "Rounded": ("materialsymbolsrounded", "default", "fill1"),
    "Sharp": ("materialsymbolssharp", "default", "fill1"),
    "Light": ("materialsymbolsrounded", "wght300", "wght300fill1"),
}

NAMES = """
shield! pin! casino! ecg_heart! widgets! key key_vertical password lock! lock_open! fingerprint visibility visibility_off
content_copy search add star! keep! folder! label credit_card! badge sticky_note_2 description terminal api database
license wifi qr_code_2 qr_code_scanner tune settings delete restore_from_trash history link language mail alternate_email
group cloud_upload sync share close check edit more_vert more_horiz arrow_back chevron_right expand_more person grid_view
view_agenda view_list palette bolt! warning! error! info timer inventory_2 local_fire_department! refresh swap_horiz
drag_handle devices backup download upload attach_file image sell filter_list sort auto_awesome! dashboard help logout
open_in_new undo done_all cancel gpp_good! gpp_bad! gpp_maybe! encrypted account_circle favorite! router nfc usb
radar visibility_lock phishing dns layers vpn_key security_key contract receipt_long emergency_home family_restroom
import_export file_open draft cloud_sync contactless content_paste keyboard text_fields abc numbers match_case
check_circle! shuffle balance schedule event dark_mode light_mode contrast mobile_lock_portrait screen_lock_portrait
vibration animation folder_open home_storage hub qr_code password_2 lock_reset lock_clock person_search mark_email_read
database_upload snooze motion_photos_on swipe touch_app inbox format_list_bulleted tag dangerous sentiment_satisfied
shopping_cart storefront shopping_bag restaurant local_cafe flight hotel luggage directions_car train directions_bus
local_shipping local_gas_station school work account_balance savings payments wallet currency_bitcoin toll
sports_esports movie music_note headphones live_tv podcasts subscriptions photo_camera videocam mic call smartphone
laptop_mac desktop_windows watch cast chat forum contacts public map location_on home apartment local_hospital
medication fitness_center pets child_care local_grocery_store sports_soccer menu_book newspaper science code bug_report
memory storage cloud hard_drive bluetooth print gavel policy verified_user admin_panel_settings support_agent
volunteer_activism celebration cake redeem rocket_launch construction water_drop eco park beach_access flag translate
brush photo_library theater_comedy smart_toy face calendar_month alarm hiking kitchen bed power electric_car
""".split()

# Only the desktop app and the extension draw these, so the phone does not carry them.
DESKTOP_NAMES = """
view_sidebar left_panel_close left_panel_open print computer extension update keyboard_command_key arrow_drop_down
chevron_left unfold_more check_box check_box_outline_blank indeterminate_check_box remove dock_to_left cloud storage
archive bookmark text_increase density_small density_medium window notifications rule account_tree move_to_inbox block
science space_dashboard segment key_off vpn_lock sync_lock lan robot_2 stacks
""".split()

ICONS = pathlib.Path("android/app/src/main/java/com/wasimaster/wmpass/ui/icons")
CACHE = pathlib.Path("target/symbols")
CACHE.mkdir(parents=True, exist_ok=True)


def fetch(family: str, symbol: str, variant: str) -> str:
    cached = CACHE / f"{family}-{symbol}-{variant}.svg"
    if not cached.exists():
        url = f"https://fonts.gstatic.com/s/i/short-term/release/{family}/{symbol}/{variant}/24px.svg"
        with urllib.request.urlopen(url, timeout=30) as response:
            cached.write_bytes(response.read())
    return " ".join(re.findall(r'\sd="([^"]+)"', cached.read_text()))


def camel(symbol: str) -> str:
    return "".join(part.capitalize() for part in symbol.split("_"))


glyphs = []  # (property, symbol, filled)
for raw in NAMES:
    name = raw.rstrip("!")
    glyphs.append((camel(name), name, False))
    if raw.endswith("!"):
        glyphs.append((camel(name) + "Filled", name, True))

failed = []


def one(style, glyph):
    family, plain, solid = STYLES[style]
    _, symbol, filled = glyph
    return fetch(family, symbol, solid if filled else plain)


with concurrent.futures.ThreadPoolExecutor(16) as pool:
    for style in STYLES:
        paths = list(pool.map(lambda g: one(style, g), glyphs))
        rows = "\n".join(f'        {i} -> "{path}"' for i, path in enumerate(paths))
        (ICONS / f"IconPaths{style}.kt").write_text(
            "// Generated by tools/icon/gen_symbols.py from Material Symbols (Apache 2.0). Do not edit.\n"
            '@file:Suppress("MaxLineLength")\n\n'
            "package com.wasimaster.wmpass.ui.icons\n\n"
            f"internal object IconPaths{style} {{\n"
            "    fun path(index: Int): String = when (index) {\n"
            f"{rows}\n"
            '        else -> ""\n'
            "    }\n}\n"
        )

wanted = ["Shield", "Key", "Search", "ContentCopy", "Settings", "Delete", "Star"]
sample = ", ".join(str(next(i for i, g in enumerate(glyphs) if g[0] == name)) for name in wanted)
getters = "\n".join(f"    val {prop}: ImageVector get() = glyph({i})" for i, (prop, _, _) in enumerate(glyphs))
(ICONS / "PassIcons.kt").write_text(f"""// Generated by tools/icon/gen_symbols.py from Material Symbols (Apache 2.0). Do not edit.
@file:Suppress("MaxLineLength")

package com.wasimaster.wmpass.ui.icons

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/**
 * Every glyph the app draws, in the style chosen under Appearance. Reading one inside a composable
 * subscribes it to that choice, so changing the style redraws everything at once. A value kept
 * somewhere outside composition keeps the style it was read in.
 */
object PassIcons {{
    private const val COUNT = {len(glyphs)}
    private val built = HashMap<String, Array<ImageVector?>>()

    /** `outlined`, `rounded`, `sharp` or `light`. */
    var style by mutableStateOf("rounded")

    private fun pathOf(style: String, index: Int): String = when (style) {{
        "rounded" -> IconPathsRounded.path(index)
        "sharp" -> IconPathsSharp.path(index)
        "light" -> IconPathsLight.path(index)
        else -> IconPathsOutlined.path(index)
    }}

    /** A few glyphs in a given style, to show what choosing it would look like. */
    fun sample(style: String): List<ImageVector> = listOf({sample}).map {{ glyph(it, style) }}

    private fun glyph(index: Int, current: String = style): ImageVector {{
        val table = built.getOrPut(current) {{ arrayOfNulls(COUNT) }}
        return table[index] ?: ImageVector.Builder(
            name = "$current$index",
            defaultWidth = 24.dp,
            defaultHeight = 24.dp,
            viewportWidth = 960f,
            viewportHeight = 960f,
        )
            .addGroup(translationY = 960f)
            .addPath(pathData = addPathNodes(pathOf(current, index)), fill = SolidColor(Color.Black))
            .clearGroup()
            .build()
            .also {{ table[index] = it }}
    }}

{getters}
}}
""")

# ---- the same glyphs for the desktop app and the extension ----

WEB = pathlib.Path("ui/src/icons")
WEB.mkdir(parents=True, exist_ok=True)
web_glyphs = [(symbol + ("_fill" if filled else ""), symbol, filled) for _, symbol, filled in glyphs]
# A desktop name the phone now carries too is already in the table.
phone_symbols = {symbol for _, symbol, _ in glyphs}
web_glyphs += [(symbol, symbol, False) for symbol in DESKTOP_NAMES if symbol not in phone_symbols]

with concurrent.futures.ThreadPoolExecutor(16) as pool:
    for style in STYLES:
        paths = list(pool.map(lambda g: one(style, g), web_glyphs))
        rows = "\n".join(f'  {key}: "{path}",' for (key, _, _), path in zip(web_glyphs, paths))
        (WEB / f"{style.lower()}.ts").write_text(
            "// Generated by tools/icon/gen_symbols.py from Material Symbols (Apache 2.0). Do not edit.\n"
            f"export default {{\n{rows}\n}} as const;\n"
        )
print("glyphs", len(glyphs), "web", len(web_glyphs))

"""The Material Symbols names the apps draw, shared by gen_symbols.py and gen_packs.py.

Names ending in `!` also get a filled twin, for the selected state of a tab. The order is the
order of the glyph tables, so a name is only ever added at the end of its list.
"""

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


def camel(symbol: str) -> str:
    return "".join(part.capitalize() for part in symbol.split("_"))


def phone_glyphs():
    """(property, symbol, filled) for every glyph the phone carries, in table order."""
    glyphs = []
    for raw in NAMES:
        name = raw.rstrip("!")
        glyphs.append((camel(name), name, False))
        if raw.endswith("!"):
            glyphs.append((camel(name) + "Filled", name, True))
    return glyphs


def web_glyphs():
    """(key, symbol, filled) for the desktop app and the extension: the phone's, then their own."""
    glyphs = phone_glyphs()
    out = [(symbol + ("_fill" if filled else ""), symbol, filled) for _, symbol, filled in glyphs]
    # A desktop name the phone now carries too is already in the table.
    phone_symbols = {symbol for _, symbol, _ in glyphs}
    out += [(symbol, symbol, False) for symbol in DESKTOP_NAMES if symbol not in phone_symbols]
    return out

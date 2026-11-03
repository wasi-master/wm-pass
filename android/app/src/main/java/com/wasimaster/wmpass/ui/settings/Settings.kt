package com.wasimaster.wmpass.ui.settings

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavGraphBuilder
import androidx.navigation.compose.composable
import com.wasimaster.wmpass.core.ImportPreview
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.QuickUnlock
import com.wasimaster.wmpass.core.SlotView
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.Accent
import com.wasimaster.wmpass.ui.theme.Look
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.AllTabs
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.ActionRow
import com.wasimaster.wmsuite.settings.ChoiceSetting
import com.wasimaster.wmsuite.settings.NavRow
import com.wasimaster.wmsuite.settings.ReorderableColumn
import com.wasimaster.wmsuite.settings.SettingsGroup
import com.wasimaster.wmsuite.settings.ToggleSetting
import com.wasimaster.wmsuite.settings.WmScreen
import com.wasimaster.wmsuite.settings.routeAccent
import kotlinx.coroutines.launch
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** Every page behind the gear. */
fun NavGraphBuilder.settingsGraph() {
    composable("settings") { SettingsRoot() }
    composable("settings/security") { SecurityScreen() }
    composable("settings/look") { LookScreen() }
    composable("settings/appearance") { AppearanceHub() }
    composable("settings/editing") { EditingHub() }
    composable("settings/tools") { ToolsHub() }
    composable("settings/trash") { TrashScreen() }
    composable("settings/shelf") { ShelfScreen() }
    composable("settings/folders") { FoldersScreen() }
    composable("settings/import") { ImportScreen() }
    composable("settings/export") { ExportScreen() }
    composable("settings/compare") { CompareScreen() }
    composable("settings/about") { AboutScreen() }
    composable("settings/breaches") { BreachScreen() }
    composable("settings/backup") { BackupScreen() }
    composable("settings/sync") { SyncScreen() }
    composable("settings/aliases") { AliasScreen() }
    composable("settings/usernames") { UsernameScreen() }
    composable("settings/inbox") { InboxScreen() }
    composable("settings/hardware") { HardwareScreen() }
    composable("settings/vaults") { VaultsScreen() }
    composable("settings/receive") { ReceiveScreen() }
    composable("settings/shortcuts") { ShortcutsScreen() }
    composable("settings/themes") { ThemesScreen() }
    composable("settings/icons") { IconsScreen() }
    composable("settings/list") { ListSettingsScreen() }
    composable("settings/navigation") { NavigationSettingsScreen() }
    composable("settings/add") { AddMenuScreen() }
    composable("settings/editor") { EditorSettingsScreen() }
    composable("settings/entry") { EntrySettingsScreen() }
    composable("settings/codes") { CodesSettingsScreen() }
    composable("settings/generator") { GeneratorSettingsScreen() }
    composable("settings/health") { HealthSettingsScreen() }
    composable("settings/file") { SettingsFileScreen() }
    composable("settings/search") { SettingsSearchScreen() }
    composable("settings/theme_edit/{id}") { entry -> ThemeEditor(entry.arguments?.getString("id").orEmpty()) }
}

@Composable
private fun back(): () -> Unit {
    val shell = LocalShell.current
    return { shell.nav.popBackStack() }
}

@Composable
fun SettingsRoot() {
    val shell = LocalShell.current
    val nav = shell.nav
    WmScreen(
        "Settings",
        onBack = back(),
        actions = {
            androidx.compose.material3.IconButton(onClick = { nav.navigate("settings/search") }) { Icon(PassIcons.Search, "Search settings") }
        },
    ) {
        SettingsGroup("Protection") {
            item {
                NavRow("Security", "Auto-lock, biometrics, duress and emergency access", route = "settings/security", accent = routeAccent("settings/security"), icon = PassIcons.Shield) { nav.navigate("settings/security") }
            }
            item {
                NavRow("Hidden shelf", "Hide items from lists and autofill", route = "settings/shelf", accent = routeAccent("settings/shelf"), icon = PassIcons.VisibilityLock) { nav.navigate("settings/shelf") }
            }
            item {
                NavRow("Breaches", "Check passwords and email addresses against known leaks", route = "settings/breaches", accent = routeAccent("settings/breaches"), icon = PassIcons.LocalFireDepartment) { nav.navigate("settings/breaches") }
            }
            item {
                NavRow("Health checks", "Which problems are listed", route = "settings/health", accent = routeAccent("settings/health"), icon = PassIcons.EcgHeart) { nav.navigate("settings/health") }
            }
        }
        SettingsGroup("Your vault") {
            item {
                NavRow("Folders", route = "settings/folders", accent = routeAccent("settings/folders"), icon = PassIcons.Folder) { nav.navigate("settings/folders") }
            }
            item {
                NavRow("Trash", route = "settings/trash", accent = routeAccent("settings/trash"), icon = PassIcons.Delete) { nav.navigate("settings/trash") }
            }
            item {
                NavRow("Import", "From 1Password, Bitwarden, Chrome, KeePass and more", route = "settings/import", accent = routeAccent("settings/import"), icon = PassIcons.Download) { nav.navigate("settings/import") }
            }
            item {
                NavRow("Export", route = "settings/export", accent = routeAccent("settings/export"), icon = PassIcons.Upload) { nav.navigate("settings/export") }
            }
        }
        SettingsGroup("Backup and sync") {
            item {
                NavRow("Backups", "Encrypted copies on a schedule", route = "settings/backup", accent = routeAccent("settings/backup"), icon = PassIcons.Backup) { nav.navigate("settings/backup") }
            }
            item {
                NavRow("Sync and accounts", "Other devices, servers, Bitwarden and KeePass", route = "settings/sync", accent = routeAccent("settings/sync"), icon = PassIcons.Sync) { nav.navigate("settings/sync") }
            }
        }
        SettingsGroup("Tools and customizing") {
            item {
                NavRow("Tools", "Email aliases, a throwaway inbox, names and comparing", route = "settings/tools", accent = routeAccent("settings/tools"), icon = PassIcons.Widgets) { nav.navigate("settings/tools") }
            }
            item {
                NavRow("Appearance", "Style, theme, icons, the list and the item page", route = "settings/appearance", accent = routeAccent("settings/appearance"), icon = PassIcons.Palette) { nav.navigate("settings/appearance") }
            }
            item {
                NavRow("Adding and editing", "The add menu, the editor and the generator", route = "settings/editing", accent = routeAccent("settings/editing"), icon = PassIcons.Edit) { nav.navigate("settings/editing") }
            }
            item {
                NavRow("Keyboard shortcuts", "For a keyboard plugged in or paired", route = "settings/shortcuts", accent = routeAccent("settings/shortcuts"), icon = PassIcons.Keyboard) { nav.navigate("settings/shortcuts") }
            }
        }
        SettingsGroup("This app") {
            item {
                NavRow("Settings file", "Export, import or reset", route = "settings/file", accent = routeAccent("settings/file"), icon = PassIcons.ImportExport) { nav.navigate("settings/file") }
            }
            item {
                NavRow("About", route = "settings/about", accent = routeAccent("settings/about"), icon = PassIcons.Info) { nav.navigate("settings/about") }
            }
        }
    }
}

/** A page of pages, so the root lists what a thing is about rather than every screen. */
@Composable
private fun SettingsHub(title: String, route: String, content: @Composable () -> Unit) {
    WmScreen(title, route = route, onBack = back(), content = content)
}

@Composable
private fun AppearanceHub() {
    val nav = LocalShell.current.nav
    SettingsHub("Appearance", "settings/appearance") {
        SettingsGroup("How it looks") {
            item {
                NavRow("Style and theme", "Style, theme, dark mode and motion", route = "settings/look", accent = routeAccent("settings/look"), icon = PassIcons.Palette) { nav.navigate("settings/look") }
            }
            item {
                NavRow("Icons", "App icons, site icons and these ones", route = "settings/icons", accent = routeAccent("settings/icons"), icon = PassIcons.Image) { nav.navigate("settings/icons") }
            }
        }
        SettingsGroup("What it shows") {
            item {
                NavRow("Vault list", "Layout, sorting, rows and gestures", route = "settings/list", accent = routeAccent("settings/list"), icon = PassIcons.ViewAgenda) { nav.navigate("settings/list") }
            }
            item {
                NavRow("Navigation", "Pages, labels and buttons", route = "settings/navigation", accent = routeAccent("settings/navigation"), icon = PassIcons.Dashboard) { nav.navigate("settings/navigation") }
            }
            item {
                NavRow("Item page", "Buttons, fields and history", route = "settings/entry", accent = routeAccent("settings/entry"), icon = PassIcons.Description) { nav.navigate("settings/entry") }
            }
            item {
                NavRow("Codes", "Size, grouping and privacy", route = "settings/codes", accent = routeAccent("settings/codes"), icon = PassIcons.Pin) { nav.navigate("settings/codes") }
            }
        }
    }
}

@Composable
private fun EditingHub() {
    val nav = LocalShell.current.nav
    SettingsHub("Adding and editing", "settings/editing") {
        SettingsGroup("New items") {
            item {
                NavRow("Add menu", "Which types are offered, and in what order", route = "settings/add", accent = routeAccent("settings/add"), icon = PassIcons.Add) { nav.navigate("settings/add") }
            }
            item {
                NavRow("Generator", "What a new password looks like", route = "settings/generator", accent = routeAccent("settings/generator"), icon = PassIcons.Casino) { nav.navigate("settings/generator") }
            }
        }
        SettingsGroup("Changing them") {
            item {
                NavRow("Editor", "What starts open or folded", route = "settings/editor", accent = routeAccent("settings/editor"), icon = PassIcons.Edit) { nav.navigate("settings/editor") }
            }
        }
    }
}

@Composable
private fun ToolsHub() {
    val nav = LocalShell.current.nav
    SettingsHub("Tools", "settings/tools") {
        SettingsGroup("Addresses and names") {
            item {
                NavRow("Email aliases", "SimpleLogin, addy.io, Firefox Relay, DuckDuckGo", route = "settings/aliases", accent = routeAccent("settings/aliases"), icon = PassIcons.AlternateEmail) { nav.navigate("settings/aliases") }
            }
            item {
                NavRow("Disposable inbox", "A throwaway address for one sign-up", route = "settings/inbox", accent = routeAccent("settings/inbox"), icon = PassIcons.Inbox) { nav.navigate("settings/inbox") }
            }
            item {
                NavRow("Username checker", "See which sites have a name free", route = "settings/usernames", accent = routeAccent("settings/usernames"), icon = PassIcons.PersonSearch) { nav.navigate("settings/usernames") }
            }
        }
        SettingsGroup("Passwords") {
            item {
                NavRow("Compare two passwords", route = "settings/compare", accent = routeAccent("settings/compare"), icon = PassIcons.Balance) { nav.navigate("settings/compare") }
            }
        }
    }
}

/** The Tools tab: the same tools, a thumb away, for anyone who puts them in the dock. */
@Composable
fun ToolsBody() {
    val nav = LocalShell.current.nav
    SettingsGroup {
        item {
            NavRow("Email aliases", icon = PassIcons.AlternateEmail) { nav.navigate("settings/aliases") }
        }
        item {
            NavRow("Disposable inbox", icon = PassIcons.Inbox) { nav.navigate("settings/inbox") }
        }
        item {
            NavRow("Username checker", icon = PassIcons.PersonSearch) { nav.navigate("settings/usernames") }
        }
        item {
            NavRow("Breaches", icon = PassIcons.LocalFireDepartment) { nav.navigate("settings/breaches") }
        }
        item {
            NavRow("Compare two passwords", icon = PassIcons.Balance) { nav.navigate("settings/compare") }
        }
        item {
            NavRow("Settings", icon = PassIcons.Settings) { nav.navigate("settings") }
        }
    }
}

// ---- security ----

@Composable
private fun SecurityScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val context = LocalContext.current
    val revision by Pass.revision.collectAsStateWithLifecycle()
    var slots by remember { mutableStateOf<List<SlotView>>(emptyList()) }
    var sheet by rememberSaveable { mutableStateOf<String?>(null) }
    var refresh by remember { mutableStateOf(0) }
    LaunchedEffect(revision, refresh) { slots = runCatching { Pass.slots() }.getOrDefault(emptyList()) }
    // Keyboard and watch access start on once their companion is really there; the toggles show what was decided.
    LaunchedEffect(Unit) {
        if (com.wasimaster.wmpass.system.KeyboardProvider.installed(context)) prefs.switchOnFirstTime(prefs.keyboardAccess)
        prefs.wearAccess.load()
    }
    val quick = remember { runCatching { QuickUnlock(context) }.getOrNull() }
    var quickOn by remember { mutableStateOf(quick?.enrolledFile() != null) }
    val keyFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            pendingKeyFile = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
            sheet = "keyfile"
        }
    }
    WmScreen("Security", route = "settings/security", onBack = back()) {
        SettingsGroup("Locking") {
            item {
                ChoiceSetting(
                    "Lock after leaving the app",
                    options = listOf(0 to "At once", 60 to "1 min", 300 to "5 min", 1800 to "30 min", -1 to "Never"),
                    selected = prefs.lockAfter,
                    icon = PassIcons.LockClock,
                ) { prefs.updateLockAfter(it) }
            }
            item {
                ToggleSetting(
                    "Unlock with fingerprint",
                    "Your phone's secure hardware holds the key. A new fingerprint added to the phone turns this off.",
                    quickOn,
                    icon = PassIcons.Fingerprint,
                    enabled = quick != null,
                ) { on ->
                    if (!on) {
                        quick?.disable()
                        quickOn = false
                    } else {
                        val activity = context as? FragmentActivity ?: return@ToggleSetting
                        val file = Pass.currentFileName ?: return@ToggleSetting
                        val cipher = runCatching { quick?.cipherForEnrolling(file) }.getOrNull() ?: return@ToggleSetting
                        val prompt = BiometricPrompt(
                            activity,
                            ContextCompat.getMainExecutor(context),
                            object : BiometricPrompt.AuthenticationCallback() {
                                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                                    shell.launch {
                                        quick?.finishEnrolling(file, result.cryptoObject!!.cipher!!, Pass.keyForWrapping())
                                        quickOn = true
                                        shell.haptics.confirm()
                                    }
                                }
                            },
                        )
                        prompt.authenticate(
                            BiometricPrompt.PromptInfo.Builder()
                                .setTitle("Unlock WM Pass with your fingerprint")
                                .setNegativeButtonText("Cancel")
                                .setAllowedAuthenticators(androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG)
                                .build(),
                            BiometricPrompt.CryptoObject(cipher),
                        )
                    }
                }
            }
            item {
                ToggleSetting(
                    "Block screenshots",
                    "Also hides the app in the recent apps list.",
                    prefs.screenProtection,
                    icon = PassIcons.ScreenLockPortrait,
                ) { prefs.updateScreenProtection(it) }
            }
            item {
                ChoiceSetting(
                    "Clear the clipboard after",
                    options = listOf(15 to "15 s", 30 to "30 s", 60 to "1 min", 120 to "2 min"),
                    selected = prefs.clipboardSeconds,
                    icon = PassIcons.ContentPaste,
                ) { prefs.updateClipboard(it) }
            }
            item {
                ToggleSetting("Lock when the screen goes off", null, prefs.lockScreenOff.value, icon = PassIcons.ScreenLockPortrait) { prefs.lockScreenOff.set(it) }
            }
        }
        SettingsGroup("Master password") {
            item {
                NavRow("Change it", "Old backups still open with the old password", icon = PassIcons.Password) { sheet = "change" }
            }
            item {
                NavRow("Change it and rekey", "A new vault key too, so old copies of the file cannot follow new changes", icon = PassIcons.LockReset) { sheet = "rekey" }
            }
        }
        SettingsGroup("Unlock methods") {
            slots.forEach { slot ->
                item {
                    NavRow(slot.label.ifEmpty { "Password ${slot.index + 1}" }, icon = PassIcons.Key) { sheet = "slot:${slot.index}" }
                }
            }
            item {
                NavRow("Add a password", icon = PassIcons.Add) { sheet = "add" }
            }
            item {
                NavRow("Add a key file", "Both the password and the file will be needed", icon = PassIcons.FileOpen) { keyFile.launch(arrayOf("*/*")) }
            }
            item {
                NavRow("Add a security key", "YubiKey or another FIDO2 key, by USB or NFC", icon = PassIcons.SecurityKey) { shell.nav.navigate("settings/hardware") }
            }
        }
        SettingsGroup("Duress") {
            item {
                NavRow("Wipe password", "Erases the vault on this device. Looks like a wrong password.", icon = PassIcons.Dangerous) { sheet = "duress:wipe" }
            }
            item {
                NavRow("Lockout password", "Refuses every attempt for an hour.", icon = PassIcons.Lock) { sheet = "duress:lock" }
            }
            item {
                NavRow("Decoy vault", "A separate vault with its own password.", icon = PassIcons.Layers) { sheet = "decoy" }
            }
        }
        SettingsGroup("Autofill and panic") {
            item {
                ToggleSetting("Inline suggestions", "Show saved logins in the keyboard's suggestion row.", prefs.autofillInline.value, icon = PassIcons.Keyboard) { prefs.autofillInline.set(it) }
            }
            item {
                ToggleSetting("Offer to save passwords", "Ask after you sign in with a new or changed password.", prefs.autofillSave.value, icon = PassIcons.Key) { prefs.autofillSave.set(it) }
            }
            item {
                ToggleSetting(
                    "Let WM Keyboard fill",
                    "Its WM Pass tool can list your logins and type them while the vault is open, and its settings can fill API keys from here or save new ones. A password it types passes through the keyboard. On by default when WM Keyboard is installed.",
                    prefs.keyboardAccess.value,
                    icon = PassIcons.Keyboard,
                ) { prefs.keyboardAccess.set(it) }
            }
            if (com.wasimaster.wmpass.BuildConfig.WATCH) {
                item {
                    ToggleSetting(
                        "Show on my watch",
                        "While the vault is open here, your paired watch can show one-time codes and favorites. The watch keeps nothing. On once a watch with WM Pass asks.",
                        prefs.wearAccess.value,
                        icon = PassIcons.Key,
                    ) { prefs.wearAccess.set(it) }
                }
            }
            item {
                NavRow("Autofill service", "Set WM Pass as this device's autofill service", icon = PassIcons.TextFields) {
                    runCatching {
                        context.startActivity(
                            android.content.Intent(android.provider.Settings.ACTION_REQUEST_SET_AUTOFILL_SERVICE)
                                .setData(android.net.Uri.parse("package:${context.packageName}")),
                        )
                    }
                }
            }
            item {
                ChoiceSetting(
                    "When a panic app signals",
                    subtitle = if (prefs.panicAction == "wipe") "Wipes only for ${prefs.panicTrustedApp ?: "the app you name"}. Others just lock." else "Locks and refuses passwords for an hour.",
                    options = listOf("lock" to "Lock", "wipe" to "Wipe"),
                    selected = prefs.panicAction,
                    icon = PassIcons.Dangerous,
                ) { action -> if (action == "wipe") sheet = "panic" else prefs.updatePanic("lock", null) }
            }
        }
        SettingsGroup("Emergency access") {
            item {
                NavRow("Emergency kit", "Split your key between trusted people", icon = PassIcons.FamilyRestroom) { sheet = "kit" }
            }
        }
    }
    val current = sheet
    when {
        current == "change" -> ChangePasswordSheet(rekey = false) { sheet = null }
        current == "rekey" -> ChangePasswordSheet(rekey = true) { sheet = null }
        current == "add" -> AddPasswordSheet("Add a password", "Opens the same vault.", "open") { sheet = null; refresh++ }
        current == "keyfile" -> AddPasswordSheet("Password for the key file", "This password and the file together will open the vault.", "open", keyFile = true) { sheet = null; refresh++ }
        current == "duress:wipe" -> AddPasswordSheet(
            "Wipe password",
            "Entering this erases the vault on this device. Flash storage may keep traces, and backups and synced copies are not affected.",
            "wipe",
        ) { sheet = null; refresh++ }
        current == "duress:lock" -> AddPasswordSheet("Lockout password", "Refuses every attempt for an hour.", "lock") { sheet = null; refresh++ }
        current == "decoy" -> DecoySheet { sheet = null }
        current == "kit" -> KitSheet { sheet = null }
        current == "panic" -> {
            val app = rememberTextFieldState(prefs.panicTrustedApp.orEmpty())
            PassSheet({ sheet = null }, "Wipe on a panic signal") {
                Text(
                    "Type the package name of the panic app you use, such as info.guardianproject.ripple. A signal from it erases this phone's vault with no warning. Backups and synced copies stay.",
                    color = palette.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(14.dp))
                SheetField(app, "Package name")
                Spacer(Modifier.height(14.dp))
                QuietButton("Wipe on its signal", Modifier.fillMaxWidth(), PassIcons.Dangerous, palette.danger, enabled = app.text.isNotBlank()) {
                    prefs.updatePanic("wipe", app.text.toString().trim())
                    sheet = null
                }
            }
        }
        current?.startsWith("slot:") == true -> {
            val index = current.removePrefix("slot:").toInt()
            ConfirmSheet(
                "Remove this way in?",
                "It will stop opening the vault. The file keeps at least one other way in.",
                "Remove",
                onDismiss = { sheet = null },
            ) {
                shell.launch {
                    runCatching { Pass.change("slot.remove") { put("index", index) } }
                    refresh++
                }
            }
        }
    }
}

/** Key file bytes between the picker and the sheet that uses them. */
private var pendingKeyFile: ByteArray? = null

@Composable
private fun ChangePasswordSheet(rekey: Boolean, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val old = rememberTextFieldState()
    val new = rememberTextFieldState()
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    PassSheet(onDismiss, if (rekey) "Change it and rekey" else "Change your password") {
        Text(
            if (rekey) {
                "The vault gets a new key and is encrypted again. Any other way in stops working until you add it back."
            } else {
                "The vault key stays the same, so a backup made earlier still opens with the old password."
            },
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(14.dp))
        PasswordPill(old, "Current password")
        Spacer(Modifier.height(10.dp))
        PasswordPill(new, "New password")
        error?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
        }
        Spacer(Modifier.height(16.dp))
        PrimaryButton(if (busy) "Working…" else "Change", Modifier.fillMaxWidth(), enabled = !busy && old.text.isNotEmpty() && new.text.isNotEmpty()) {
            busy = true
            shell.launch {
                val current = old.text.toString().toByteArray()
                val next = new.text.toString().toByteArray()
                try {
                    if (!Pass.verify(current)) {
                        error = "That is not the current password."
                        shell.haptics.reject()
                    } else if (rekey) {
                        Pass.rekey(next)
                        shell.haptics.confirm()
                        onDismiss()
                    } else {
                        val slot = Pass.slots().firstOrNull()?.index ?: 0
                        Pass.change("vault.change_password", secret = next) { put("index", slot) }
                        shell.haptics.confirm()
                        onDismiss()
                    }
                } catch (failure: NativeFailure) {
                    error = failure.message
                } finally {
                    current.fill(0)
                    next.fill(0)
                    busy = false
                }
            }
        }
    }
}

@Composable
private fun AddPasswordSheet(title: String, body: String, action: String, keyFile: Boolean = false, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val field = rememberTextFieldState()
    val label = rememberTextFieldState()
    var error by remember { mutableStateOf<String?>(null) }
    PassSheet(onDismiss, title) {
        Text(body, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(14.dp))
        SheetField(label, "A name only you will see")
        Spacer(Modifier.height(10.dp))
        PasswordPill(field, "Password")
        error?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
        }
        Spacer(Modifier.height(16.dp))
        PrimaryButton("Add", Modifier.fillMaxWidth(), enabled = field.text.isNotEmpty()) {
            shell.launch {
                val secret = field.text.toString().toByteArray()
                val file = if (keyFile) pendingKeyFile else null
                try {
                    Pass.change("slot.add", secret = secret, blobs = listOfNotNull(file)) {
                        put("label", label.text.toString())
                        put("action", action)
                        if (file != null) put("factor", buildJsonObject { put("kind", "key_file") })
                    }
                    shell.haptics.confirm()
                    onDismiss()
                } catch (failure: NativeFailure) {
                    error = failure.message
                    shell.haptics.reject()
                } finally {
                    secret.fill(0)
                    file?.fill(0)
                    pendingKeyFile = null
                }
            }
        }
    }
}

@Composable
private fun DecoySheet(onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val field = rememberTextFieldState()
    val name = rememberTextFieldState("Personal")
    PassSheet(onDismiss, "A decoy vault") {
        Text(
            "A second vault, opened by its own password. The door looks the same either way and nothing in the file says it exists. This resists a quick look under pressure. It does not stand up to forensics, or to someone who has an older copy of your files.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(14.dp))
        SheetField(name, "Its name")
        Spacer(Modifier.height(10.dp))
        PasswordPill(field, "Its password")
        Spacer(Modifier.height(16.dp))
        PrimaryButton("Make it", Modifier.fillMaxWidth(), enabled = field.text.isNotEmpty()) {
            shell.launch {
                val secret = field.text.toString().toByteArray()
                Pass.createHidden(name.text.toString().ifBlank { "Personal" }, secret)
                secret.fill(0)
                shell.haptics.confirm()
                onDismiss()
            }
        }
    }
}

@Composable
private fun KitSheet(onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    var total by remember { mutableStateOf(3) }
    var threshold by remember { mutableStateOf(2) }
    var shares by remember { mutableStateOf<List<String>?>(null) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { uri ->
        val list = shares
        if (uri != null && list != null) {
            val context = shell.nav.context
            context.contentResolver.openOutputStream(uri)?.use { out ->
                out.write(
                    buildString {
                        append("WM Pass emergency kit\n\nGive each person one share. Any $threshold of them, together with a copy of the vault file, can open it. They can do so at once: the waiting period is a promise between you, not something the software can enforce.\n\n")
                        list.forEachIndexed { index, share -> append("Share ${index + 1}\n$share\n\n") }
                    }.toByteArray(),
                )
            }
        }
    }
    PassSheet(onDismiss, "Emergency kit") {
        Text(
            "Your key is split into shares. Any $threshold of the $total people can open your vault together, and fewer learn nothing. They can do it at once: any waiting period is a promise between you, not something software can enforce.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(14.dp))
        val list = shares
        if (list == null) {
            ChoiceSetting("People", options = (2..7).map { it to it.toString() }, selected = total) {
                total = it
                threshold = threshold.coerceAtMost(it)
            }
            ChoiceSetting("Needed together", options = (2..total).map { it to it.toString() }, selected = threshold) { threshold = it }
            Spacer(Modifier.height(12.dp))
            PrimaryButton("Make the shares", Modifier.fillMaxWidth()) {
                shell.launch {
                    val reply = Pass.change("emergency.kit") {
                        put("threshold", threshold)
                        put("total", total)
                    }
                    shares = reply.secret?.consumeText()?.split('\n')
                }
            }
        } else {
            Text("Save this file, print it, and delete the file once each person has their share.", color = colors.text, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(12.dp))
            PrimaryButton("Save the kit", Modifier.fillMaxWidth(), icon = PassIcons.Download) { save.launch("wm-pass-emergency-kit.txt") }
        }
    }
}

// ---- look ----

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun LookScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    WmScreen("Style and theme", route = "settings/look", onBack = back()) {
        SettingsGroup("Interface") {
            item {
                ChoiceSetting(
                    "Style",
                    subtitle = if (prefs.uiStyle == "glass") "Blur, glow and larger animations." else "Plain surfaces and the usual Android layout.",
                    options = listOf("standard" to "Standard", "glass" to "Glass"),
                    selected = prefs.uiStyle,
                    icon = PassIcons.Layers,
                ) { prefs.updateUiStyle(it) }
            }
            item {
                NavRow("Theme", "Dracula, Catppuccin, Nord, Solarized and more, or your own", icon = PassIcons.Palette) { shell.nav.navigate("settings/themes") }
            }
            item {
                ChoiceSetting(
                    "Dark mode",
                    options = listOf(Look.SYSTEM to "System", Look.LIGHT to "Light", Look.DARK to "Dark", Look.OBSIDIAN to "Black"),
                    selected = prefs.look,
                    icon = PassIcons.Contrast,
                ) { prefs.updateLook(it) }
            }
            if (prefs.uiStyle == "glass") item {
                FlowRow(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Accent.entries.forEach { accent ->
                        Box(
                            Modifier
                                .size(46.dp)
                                .clip(CircleShape)
                                .background(Brush.linearGradient(listOf(accent.from, accent.to)))
                                .then(if (accent == prefs.accent) Modifier.border(3.dp, colors.text, CircleShape) else Modifier)
                                .clickable(onClickLabel = accent.label) {
                                    shell.haptics.tick()
                                    prefs.updateAccent(accent)
                                },
                        )
                    }
                }
            }
        }
        SettingsGroup("Motion and touch") {
            item {
                ToggleSetting("Animations", "Turn off for shorter transitions.", prefs.richMotion.value, icon = PassIcons.Animation) { prefs.updateRichMotion(it) }
            }
            item {
                ToggleSetting("Vibration", "A small tap when something happens.", prefs.haptics.value, icon = PassIcons.Vibration) { prefs.haptics.set(it) }
            }
        }
    }
}

// ---- lists ----

@Composable
private fun EntryLine(entry: Summary, onClick: () -> Unit) {
    val colors = palette
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SiteTile(entry.title, entry.site, entry.kind, size = 42.dp, label = entry.color, icon = entry.icon)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold), maxLines = 1)
            if (entry.subtitle.isNotEmpty()) Text(entry.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
        }
    }
}

@Composable
private fun TrashScreen() {
    val shell = LocalShell.current
    val colors = palette
    val revision by Pass.revision.collectAsStateWithLifecycle()
    var items by remember { mutableStateOf<List<Summary>>(emptyList()) }
    var days by remember { mutableStateOf(30) }
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(revision) {
        items = runCatching { Pass.trashed() }.getOrDefault(emptyList())
        days = runCatching { Pass.info().trashDays }.getOrDefault(30)
    }
    WmScreen("Trash", route = "settings/trash", onBack = back()) {
        SettingsGroup {
            item {
                ChoiceSetting(
                    "Keep deleted items for",
                    options = (listOf(7, 30, 90, 365) + days).distinct().sorted().map { it to "$it days" },
                    selected = days,
                    icon = PassIcons.Timer,
                ) { value ->
                    days = value
                    shell.launch { Pass.change("vault.prefs") { put("trash_days", value) } }
                }
            }
        }
        if (items.isEmpty()) {
            Text("Nothing in the trash.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(24.dp))
        } else {
            SettingsGroup {
                items.forEach { entry ->
                    item {
                        EntryLine(entry) { shell.open(entry.id) }
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
            QuietButton("Empty the trash", Modifier.fillMaxWidth().padding(horizontal = 16.dp), PassIcons.Delete, colors.danger) { confirm = true }
        }
    }
    if (confirm) {
        ConfirmSheet("Empty the trash?", "These ${items.size} items and their attachments will be deleted for good.", "Delete them for good", onDismiss = { confirm = false }) {
            shell.launch {
                Pass.change("entries.bulk") {
                    put("op", "purge")
                    put("ids", kotlinx.serialization.json.buildJsonArray { items.forEach { add(kotlinx.serialization.json.JsonPrimitive(it.id)) } })
                }
            }
        }
    }
}

@Composable
private fun ShelfScreen() {
    val shell = LocalShell.current
    val colors = palette
    val revision by Pass.revision.collectAsStateWithLifecycle()
    val field = rememberTextFieldState()
    var open by remember { mutableStateOf(false) }
    var items by remember { mutableStateOf<List<Summary>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(revision, open) {
        open = runCatching { Pass.info().shelfOpen }.getOrDefault(false)
        if (open) items = runCatching { Pass.search("hidden") }.getOrDefault(emptyList())
    }
    WmScreen("Hidden shelf", route = "settings/shelf", onBack = back()) {
        if (!open) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                Text(
                    "Entries on the shelf stay out of every list, search and autofill until you open it with your password. It closes again when the vault locks.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(horizontal = 4.dp),
                )
                Spacer(Modifier.height(16.dp))
                PasswordPill(field, "Master password")
                error?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
                }
                Spacer(Modifier.height(12.dp))
                PrimaryButton("Open the shelf", Modifier.fillMaxWidth(), enabled = field.text.isNotEmpty()) {
                    shell.launch {
                        val secret = field.text.toString().toByteArray()
                        try {
                            Pass.change("shelf.open", secret = secret)
                            open = true
                            shell.haptics.confirm()
                        } catch (failure: NativeFailure) {
                            error = failure.message
                            shell.haptics.reject()
                        } finally {
                            secret.fill(0)
                        }
                    }
                }
            }
        } else {
            if (items.isEmpty()) {
                Text("The shelf is empty.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(24.dp))
            } else {
                SettingsGroup {
                    items.forEach { entry ->
                        item {
                            EntryLine(entry) { shell.open(entry.id) }
                        }
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
            QuietButton("Close the shelf", Modifier.fillMaxWidth().padding(horizontal = 16.dp), PassIcons.VisibilityLock) {
                shell.launch {
                    Pass.change("shelf.close")
                    open = false
                }
            }
        }
    }
}

@Composable
private fun FoldersScreen() {
    val shell = LocalShell.current
    val colors = palette
    val folders by Pass.folders.collectAsStateWithLifecycle()
    val entries by Pass.entries.collectAsStateWithLifecycle()
    var editing by remember { mutableStateOf<String?>(null) }
    var adding by remember { mutableStateOf(false) }
    WmScreen("Folders", route = "settings/folders", onBack = back()) {
        SettingsGroup {
            folders.forEach { folder ->
                val count = entries.count { it.folder == folder.id }
                item {
                    NavRow(folder.name, value = count.toString(), icon = PassIcons.Folder) { editing = folder.id }
                }
            }
            item {
                NavRow("New folder", icon = PassIcons.Add) { adding = true }
            }
        }
    }
    if (adding) {
        NameSheet("New folder", "", onDismiss = { adding = false }) { name ->
            shell.launch { Pass.change("folder.add") { put("name", name) } }
        }
    }
    val folder = folders.firstOrNull { it.id == editing }
    if (folder != null) {
        PassSheet({ editing = null }, folder.name) {
            val name = rememberTextFieldState(folder.name)
            SheetField(name, "Name")
            Spacer(Modifier.height(14.dp))
            PrimaryButton("Rename", Modifier.fillMaxWidth(), enabled = name.text.isNotBlank()) {
                shell.launch {
                    Pass.change("folder.rename") {
                        put("id", folder.id)
                        put("name", name.text.toString().trim())
                    }
                    editing = null
                }
            }
            Spacer(Modifier.height(10.dp))
            QuietButton("Remove the folder", Modifier.fillMaxWidth(), PassIcons.Delete, colors.danger) {
                shell.launch {
                    Pass.change("folder.remove") { put("id", folder.id) }
                    editing = null
                }
            }
            Spacer(Modifier.height(6.dp))
            Text("What is in it stays in the vault, in no folder.", color = colors.textMid, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 6.dp))
        }
    }
}

@Composable
fun NameSheet(title: String, initial: String, onDismiss: () -> Unit, onDone: (String) -> Unit) {
    val name = remember { TextFieldState(initial) }
    PassSheet(onDismiss, title) {
        SheetField(name, "Name")
        Spacer(Modifier.height(14.dp))
        PrimaryButton("Done", Modifier.fillMaxWidth(), enabled = name.text.isNotBlank()) {
            onDone(name.text.toString().trim())
            onDismiss()
        }
    }
}

// ---- import and export ----

private val ImportFormats = "1Password, Apple Passwords, Bitwarden, Chrome, Dashlane, Enpass, Firefox, KeePass and KeePassXC, Keeper, LastPass, NordPass, Proton Pass, RoboForm and SafeInCloud. Authenticators: Aegis, 2FAS, andOTP, Ente, FreeOTP+ and Raivo files, and Google Authenticator's export codes, scanned or from a screenshot."

/** Each app's export steps, on the website. */
private const val ExportGuide = "https://wmpass.pages.dev/guides/import-export/#exporting-from-your-old-app"

@Composable
private fun ImportScreen() {
    val shell = LocalShell.current
    val colors = palette
    val context = LocalContext.current
    var preview by remember { mutableStateOf<ImportPreview?>(null) }
    var bytes by remember { mutableStateOf<ByteArray?>(null) }
    var needsPassword by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var result by remember { mutableStateOf<String?>(null) }
    val password = rememberTextFieldState()
    suspend fun read() {
        val data = bytes ?: return
        val secret = password.text.toString().toByteArray()
        try {
            val reply = Pass.plainCall("import.read", secret = secret, blobs = listOf(data))
            preview = com.wasimaster.wmpass.core.Engine.json.decodeFromJsonElement(ImportPreview.serializer(), reply.json)
            needsPassword = false
            error = null
        } catch (failure: NativeFailure) {
            needsPassword = failure.code == "needs_password" || failure.code == "wrong_password" || failure.message.orEmpty().contains("password", ignoreCase = true)
            error = if (needsPassword && password.text.isEmpty()) null else failure.message
        } finally {
            secret.fill(0)
        }
    }
    // Google Authenticator's export links, scanned or read from pictures. A long list comes as
    // several codes, and one import takes them all.
    var codes by remember { mutableStateOf(emptyList<String>()) }
    suspend fun takeCodes(found: List<String>) {
        val links = found.filter { it.startsWith("otpauth") }
        if (links.isEmpty()) {
            error = if (found.isEmpty()) "No QR code was found in that picture." else "That code is not an export from an authenticator app."
            return
        }
        codes = (codes + links).distinct()
        bytes?.fill(0)
        bytes = codes.joinToString("\n").toByteArray()
        preview = null
        result = null
        read()
    }
    fun scanCode() {
        error = null
        shell.scanTarget = { text -> shell.launch { takeCodes(listOf(text)) } }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            shell.launch {
                error = null
                // A screenshot or photo of an export code, rather than an export file.
                if (context.contentResolver.getType(uri)?.startsWith("image/") == true) {
                    takeCodes(com.wasimaster.wmpass.ui.vault.qrCodesIn(context, uri))
                    return@launch
                }
                codes = emptyList()
                bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                preview = null
                result = null
                read()
            }
        }
    }
    val browser = androidx.compose.ui.platform.LocalUriHandler.current
    var savedWifi by remember { mutableStateOf(false) }
    if (savedWifi) {
        com.wasimaster.wmpass.ui.vault.SavedWifiSheet({ savedWifi = false }, onAddAll = { networks ->
            shell.launch {
                networks.forEach { network ->
                    val id = Pass.change("entry.create") {
                        put("kind", "wifi")
                        put("title", network.ssid)
                        put(
                            "fields",
                            kotlinx.serialization.json.buildJsonObject {
                                put("ssid", network.ssid)
                                put("security", network.security)
                                put("hidden_ssid", network.hidden)
                            },
                        )
                    }.obj["id"]!!.jsonPrimitive.content
                    if (network.password.isNotEmpty()) {
                        val bytes = network.password.toByteArray()
                        Pass.change("entry.set_secret", secret = bytes) {
                            put("id", id)
                            put("field", "password")
                        }
                        bytes.fill(0)
                    }
                }
                shell.haptics.confirm()
                result = if (networks.size == 1) "Added 1 Wi-Fi network." else "Added ${networks.size} Wi-Fi networks."
            }
        })
    }
    WmScreen("Import", route = "settings/import", onBack = back()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            val shown = preview
            when {
                result != null -> {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        GlyphTile(PassIcons.CheckCircleFilled, colors.primary, size = 44.dp)
                        Spacer(Modifier.width(14.dp))
                        Text(result!!, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                    }
                    PrimaryButton("Done", Modifier.fillMaxWidth()) { shell.nav.popBackStack() }
                }
                shown != null -> {
                    Text("${shown.count} items from ${shown.format}", color = colors.text, style = MaterialTheme.typography.titleLarge)
                    Text(shown.titles.take(6).joinToString(", ") + if (shown.count > 6) " and more" else "", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                    if (shown.skipped.isNotEmpty()) {
                        Text("${shown.skipped.size} could not be read: ${shown.skipped.take(3).joinToString(", ")}", color = colors.warn, style = MaterialTheme.typography.bodyMedium)
                    }
                    PrimaryButton("Import ${shown.count} items", Modifier.fillMaxWidth(), icon = PassIcons.Download) {
                        shell.launch {
                            val reply = Pass.change("import.apply")
                            val added = reply.obj["added"]?.jsonPrimitive?.content ?: "0"
                            val dupes = reply.obj["duplicates"]?.jsonPrimitive?.content ?: "0"
                            shell.haptics.confirm()
                            result = "Added $added. Skipped $dupes already here."
                            bytes?.fill(0)
                            bytes = null
                            codes = emptyList()
                        }
                    }
                    if (codes.isNotEmpty()) {
                        Text(
                            "${codes.size} ${if (codes.size == 1) "code" else "codes"} read. Google splits a long list across several codes, so scan each one before importing.",
                            color = colors.textMid,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        error?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                        QuietButton("Scan the next code", Modifier.fillMaxWidth(), PassIcons.QrCodeScanner, colors.primary) { scanCode() }
                    }
                    QuietButton("Choose another file", Modifier.fillMaxWidth()) {
                        shell.launch { Pass.plainCall("import.cancel") }
                        preview = null
                        codes = emptyList()
                        pick.launch(arrayOf("*/*"))
                    }
                }
                needsPassword -> {
                    Text("This file has a password", color = colors.text, style = MaterialTheme.typography.titleLarge)
                    PasswordPill(password, "The file's password")
                    error?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                    PrimaryButton("Open", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty()) { shell.launch { read() } }
                }
                else -> {
                    Text("Bring everything over", color = colors.text, style = MaterialTheme.typography.titleLarge)
                    Text(ImportFormats, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                    error?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                    PrimaryButton("Choose a file", Modifier.fillMaxWidth(), icon = PassIcons.FileOpen) { pick.launch(arrayOf("*/*")) }
                    QuietButton("Scan Google Authenticator's code", Modifier.fillMaxWidth(), PassIcons.QrCodeScanner, colors.primary) { scanCode() }
                    QuietButton("Wi-Fi saved on this phone", Modifier.fillMaxWidth(), PassIcons.Wifi, colors.primary) { savedWifi = true }
                    Text("Delete the exported file afterwards. It holds your passwords in plain text.", color = colors.textLow, style = MaterialTheme.typography.bodySmall)
                    QuietButton("How to export from your old app", Modifier.fillMaxWidth(), PassIcons.OpenInNew) {
                        runCatching { browser.openUri(ExportGuide) }
                    }
                }
            }
        }
    }
}

@Composable
private fun ExportScreen() {
    val shell = LocalShell.current
    val context = LocalContext.current
    var format by remember { mutableStateOf<String?>(null) }
    var pendingName by remember { mutableStateOf("wm-pass") }
    val password = rememberTextFieldState()
    var askingPassword by remember { mutableStateOf(false) }
    var warned by remember { mutableStateOf<String?>(null) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val chosen = format
        if (uri != null && chosen != null) {
            shell.launch {
                val secret = if (chosen == "kdbx") password.text.toString().toByteArray() else null
                val reply = Pass.plainCall("export.file", secret = secret) { put("format", chosen) }
                context.contentResolver.openOutputStream(uri)?.use { out ->
                    if (reply.blobs.isNotEmpty()) out.write(reply.blobs[0]) else reply.secret?.consume { out.write(it) }
                }
                secret?.fill(0)
                shell.haptics.confirm()
            }
        }
    }
    val sealed = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) {
            shell.launch {
                val file = Pass.sealedFile()
                context.contentResolver.openOutputStream(uri)?.use { it.write(file) }
                shell.haptics.confirm()
            }
        }
    }
    WmScreen("Export", route = "settings/export", onBack = back()) {
        SettingsGroup("Stays encrypted") {
            item {
                NavRow("WM Pass vault", "The vault file itself, sealed. Opens with your password.", icon = PassIcons.Encrypted) {
                    sealed.launch("vault.wmvault")
                }
            }
            item {
                NavRow("KeePass database", "A new .kdbx file with a password you choose.", icon = PassIcons.Database) {
                    format = "kdbx"
                    askingPassword = true
                }
            }
        }
        SettingsGroup("Plain text") {
            listOf(
                Triple("bitwarden", "Bitwarden JSON", "wm-pass-bitwarden.json"),
                Triple("csv", "CSV", "wm-pass.csv"),
                Triple("html", "Printable page", "wm-pass.html"),
            ).forEach { (id, label, name) ->
                item {
                    NavRow(label, "Anyone who opens this file can read every password.", icon = PassIcons.Warning) {
                        format = id
                        pendingName = name
                        warned = label
                    }
                }
            }
        }
    }
    warned?.let { label ->
        ConfirmSheet(
            "Export without encryption?",
            "$label holds every password in plain text. Keep it off shared storage and delete it when you are done.",
            "Export",
            danger = false,
            onDismiss = { warned = null },
        ) { save.launch(pendingName) }
    }
    if (askingPassword) {
        PassSheet({ askingPassword = false }, "Password for the new file") {
            PasswordPill(password, "Password")
            Spacer(Modifier.height(14.dp))
            PrimaryButton("Export", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty()) {
                askingPassword = false
                save.launch("wm-pass.kdbx")
            }
        }
    }
}

// ---- small tools ----

@Composable
private fun CompareScreen() {
    val colors = palette
    val first = rememberTextFieldState()
    val second = rememberTextFieldState()
    var result by remember { mutableStateOf<com.wasimaster.wmpass.core.CompareView?>(null) }
    LaunchedEffect(first.text, second.text) {
        if (first.text.isEmpty() || second.text.isEmpty()) {
            result = null
        } else {
            val a = first.text.toString().toByteArray()
            val b = second.text.toString().toByteArray()
            result = runCatching { Pass.compare(a, b) }.getOrNull()
            a.fill(0)
            b.fill(0)
        }
    }
    WmScreen("Compare two passwords", route = "settings/compare", onBack = back()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            PasswordPill(first, "First password")
            result?.let { Side(it.first, it.winner == "first") }
            PasswordPill(second, "Second password")
            result?.let { Side(it.second, it.winner == "second") }
            result?.let { compared ->
                Text(
                    when (compared.winner) {
                        "even" -> "About the same."
                        else -> "The ${compared.winner} is about %,.0f times harder to guess.".format(compared.factor)
                    },
                    color = colors.text,
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(start = 6.dp, top = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun Side(view: com.wasimaster.wmpass.core.StrengthView, winner: Boolean) {
    val colors = palette
    Column(Modifier.padding(horizontal = 6.dp)) {
        com.wasimaster.wmpass.ui.parts.StrengthBar(view.score, Modifier.fillMaxWidth())
        Spacer(Modifier.height(6.dp))
        Text("Would hold for ${view.crackSlowHash}", color = if (winner) colors.primary else colors.textMid, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun AboutScreen() {
    val colors = palette
    WmScreen("About", route = "settings/about", onBack = back()) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("WM Pass ${com.wasimaster.wmpass.BuildConfig.VERSION_NAME}", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Text("Part of the WM Suite. Free and open source under the MIT license. It sends nothing anywhere unless you turn on a feature that has to.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
        }
    }
}

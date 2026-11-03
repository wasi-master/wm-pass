package com.wasimaster.wmpass.ui.door

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicSecureTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.KeyboardActionHandler
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.TextObfuscationMode
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.LineBreak
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.app.LaunchGuard
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.system.AccountRef
import com.wasimaster.wmpass.system.Accounts
import com.wasimaster.wmpass.system.accountHost
import com.wasimaster.wmpass.system.Destination
import com.wasimaster.wmpass.system.HardwareKeys
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.vault.PassSheet
import kotlinx.serialization.json.buildJsonObject
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.NeededFactor
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.QuickUnlock
import com.wasimaster.wmpass.core.StrengthView
import com.wasimaster.wmpass.core.UnlockResult
import com.wasimaster.wmpass.ui.DoorShieldCentre
import com.wasimaster.wmpass.ui.DoorShieldCentreDp
import com.wasimaster.wmpass.ui.fx.Aurora
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.SecretText
import com.wasimaster.wmpass.ui.parts.ShieldMark
import com.wasimaster.wmpass.ui.parts.ShieldState
import com.wasimaster.wmpass.ui.parts.StrengthBar
import com.wasimaster.wmpass.ui.parts.secretSemantics
import com.wasimaster.wmpass.ui.theme.MonoFamily
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.onBrand
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

/** Everything in front of the vault: the first visit, and every visit after. */
@Composable
fun DoorScreen(state: LockState, prefs: Prefs) {
    val accounts by Accounts.list.collectAsStateWithLifecycle()
    var starting by remember { mutableStateOf(false) }
    com.wasimaster.wmpass.ui.NoLearningInput {
        Box(Modifier.fillMaxSize()) {
            Aurora(Modifier.fillMaxSize())
            when {
                state == LockState.NoVault && (accounts.isEmpty() || starting) -> Welcome(if (accounts.isEmpty()) null else ({ starting = false }))
                else -> Unlock(state != LockState.NoVault, accounts) { starting = true }
            }
        }
    }
    // The guide's sheet, for the door's own question. Nothing it says can open a page from here.
    com.wasimaster.wmpass.ui.help.Help.attach(LocalContext.current)
    if (com.wasimaster.wmpass.ui.help.Guide.stack.isNotEmpty()) com.wasimaster.wmpass.ui.help.HelpSheet(prefs, null)
}

/** The door's link into the guide: what a master password is, and what makes a good one. */
@Composable
private fun MasterPasswordLink(modifier: Modifier = Modifier) {
    com.wasimaster.wmpass.ui.help.Help.attach(LocalContext.current)
    if (com.wasimaster.wmpass.ui.help.Help.topic("concept.master_password") == null) return
    androidx.compose.material3.TextButton(onClick = { com.wasimaster.wmpass.ui.help.Guide.open("concept.master_password") }, modifier = modifier) {
        Text("What is a master password?")
    }
}

@Composable
private fun ShieldSlot(state: ShieldState, typed: Int = 0, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().height((DoorShieldCentreDp * 2).dp.coerceAtMost(420.dp))) {
        ShieldMark(
            state,
            Modifier
                // On the same point while there is room for it. The keyboard takes that room, so
                // the mark rises into what is left above the form, whole, and shrinks only once
                // even that is too short. Squeezed in place, its lower half was cut off.
                .layout { measurable, constraints ->
                    val side = minOf(176.dp.roundToPx(), constraints.maxHeight)
                    val mark = measurable.measure(Constraints.fixed(side, side))
                    val top = minOf((DoorShieldCentreDp - 96).dp.roundToPx(), constraints.maxHeight - side)
                    layout(constraints.maxWidth, constraints.maxHeight) {
                        mark.place((constraints.maxWidth - side) / 2, top)
                    }
                }
                // The opening starts from wherever the shield is, so it is told each time it moves.
                .onGloballyPositioned { DoorShieldCentre.floatValue = it.positionInRoot().y + it.size.height * 72f / 108f },
            typed,
        )
    }
}

/** A password field on glass, with no learning, no suggestions and no clipboard preview. */
@Composable
fun PasswordPill(
    field: TextFieldState,
    placeholder: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    imeAction: ImeAction = ImeAction.Done,
    focus: FocusRequester? = null,
    onSubmit: (() -> Unit)? = null,
    error: Boolean = false,
    trailing: @Composable (() -> Unit)? = null,
) {
    val colors = palette
    var showing by remember { mutableStateOf(false) }
    val haptics = rememberHaptics()
    // Without its own action the key does what it says: Next moves on, Done closes the keyboard.
    val keyAction = KeyboardActionHandler { performDefault -> if (onSubmit != null) onSubmit() else performDefault() }
    if (colors.flat) {
        androidx.compose.material3.OutlinedSecureTextField(
            state = field,
            modifier = modifier
                .fillMaxWidth()
                .then(if (focus != null) Modifier.focusRequester(focus) else Modifier)
                .secretSemantics(field, showing),
            enabled = enabled,
            isError = error,
            textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp),
            label = { Text(placeholder) },
            trailingIcon = {
                androidx.compose.material3.IconButton(onClick = { showing = !showing }) {
                    Icon(if (showing) PassIcons.VisibilityOff else PassIcons.Visibility, if (showing) "Hide password" else "Show password")
                }
            },
            textObfuscationMode = if (showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = imeAction),
            onKeyboardAction = keyAction,
        )
        return
    }
    Row(
        modifier
            .fillMaxWidth()
            .height(60.dp)
            .glass(CircleShape)
            .padding(start = 22.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f)) {
            if (field.text.isEmpty()) {
                Text(placeholder, color = colors.textLow, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            }
            BasicSecureTextField(
                state = field,
                enabled = enabled,
                modifier = Modifier
                    .fillMaxWidth()
                    .then(if (focus != null) Modifier.focusRequester(focus) else Modifier)
                    .secretSemantics(field, showing),
                textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 17.sp, color = colors.text),
                cursorBrush = SolidColor(colors.primary),
                textObfuscationMode = if (showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Password,
                    autoCorrectEnabled = false,
                    imeAction = imeAction,
                ),
                onKeyboardAction = keyAction,
            )
        }
        Box(
            Modifier
                .size(44.dp)
                .clip(CircleShape)
                .clickable {
                    haptics.tick()
                    showing = !showing
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (showing) PassIcons.VisibilityOff else PassIcons.Visibility,
                if (showing) "Hide password" else "Show password",
                tint = colors.textMid,
                modifier = Modifier.size(20.dp),
            )
        }
        trailing?.invoke()
    }
}

/** A round button with the brand's gradient: the way through. */
@Composable
private fun GoButton(enabled: Boolean, onClick: () -> Unit) {
    val colors = palette
    val haptics = rememberHaptics()
    Box(
        Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(
                if (enabled) Brush.linearGradient(listOf(colors.brandFrom, colors.brandTo))
                else SolidColor(colors.surface3),
            )
            .clickable(enabled = enabled) {
                haptics.click()
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            PassIcons.ChevronRight,
            "Unlock",
            tint = if (enabled) colors.onBrand else colors.textLow,
            modifier = Modifier.size(24.dp),
        )
    }
}

// ---- every visit after the first ----

private fun formatWait(ms: Long): String {
    val seconds = (ms / 1000).coerceAtLeast(1)
    return if (seconds >= 60) "${seconds / 60} min ${seconds % 60} s" else "$seconds s"
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Unlock(hasLocal: Boolean, accounts: List<AccountRef>, onStartLocal: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val haptics = rememberHaptics()
    val scope = rememberCoroutineScope()
    val field = rememberTextFieldState()
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    var shield by remember { mutableStateOf(ShieldState.IDLE) }
    var message by remember { mutableStateOf<String?>(null) }
    var typed by remember { mutableIntStateOf(0) }
    var retryAt by remember { mutableStateOf(Pass.retryAt()) }
    var remaining by remember { mutableStateOf(0L) }
    var needed by remember { mutableStateOf<List<NeededFactor>?>(null) }
    // An account just added comes chosen, with its password asked for straight away.
    var source by remember {
        val next = Accounts.takeSignInNext()?.let { id -> accounts.firstOrNull { it.id == id } }
        mutableStateOf(next ?: if (hasLocal) null else accounts.firstOrNull())
    }
    // After a reset the look is standard, so only the standard door below says why.
    val notice = remember { LaunchGuard.takeNotice() }
    val codeField = rememberTextFieldState()
    var asksCode by remember { mutableStateOf(false) }
    // Bitwarden's check on a phone it has not seen before, rather than a two-step code.
    var emailedCode by remember { mutableStateOf(false) }
    var keyFile by remember { mutableStateOf<ByteArray?>(null) }
    val pickAccountKey = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) keyFile = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
    }
    val activity = context as? FragmentActivity
    val quick = remember { runCatching { QuickUnlock(context) }.getOrNull() }
    val biometricsReady = remember(activity) {
        activity != null && quick?.enrolledFile() != null &&
            BiometricManager.from(context).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS
    }

    LaunchedEffect(retryAt) {
        while (retryAt > System.currentTimeMillis()) {
            remaining = retryAt - System.currentTimeMillis()
            delay(250)
        }
        remaining = 0
    }
    LaunchedEffect(field) {
        snapshotFlow { field.text.length }.collectLatest { typed++ }
    }

    fun submitAccount(ref: AccountRef) {
        if (shield == ShieldState.WORKING || field.text.isEmpty() || remaining > 0) return
        val password = field.text.toString().toByteArray()
        shield = ShieldState.WORKING
        message = null
        scope.launch {
            try {
                if (ref.kind == "kdbx") {
                    Accounts.openKdbx(context, ref, password, keyFile)
                } else {
                    Accounts.openBitwarden(context, ref, password, codeField.text.toString().trim().ifEmpty { null }, deviceCode = emailedCode)
                }
                field.clearText()
                keyboard?.hide()
                haptics.confirm()
                shield = ShieldState.OPENED
            } catch (failure: NativeFailure) {
                haptics.reject()
                when (failure.code) {
                    "two_factor", "new_device" -> {
                        asksCode = true
                        emailedCode = failure.code == "new_device"
                        codeField.clearText()
                        shield = ShieldState.IDLE
                        message = if (emailedCode) {
                            "Bitwarden has emailed you a code to confirm this phone. Enter it with your password."
                        } else {
                            "This account also needs a code from your authenticator."
                        }
                    }
                    // Not a guess at the password, so none of these counts towards the wait.
                    "bad_code" -> {
                        shield = ShieldState.IDLE
                        message = "That code was not accepted."
                    }
                    "network", "bitwarden" -> {
                        shield = ShieldState.IDLE
                        message = failure.message?.let { it.replaceFirstChar(Char::uppercase).trimEnd('.') + "." } ?: "The server could not be reached."
                    }
                    else -> {
                        shield = ShieldState.REFUSED
                        // Say which account and which server were tried: an account made on
                        // bitwarden.eu is unknown to bitwarden.com, which answers just as it does a wrong password.
                        message = if (ref.kind == "bitwarden" && ref.server.isEmpty()) {
                            val other = if (ref.region == "eu") "bitwarden.com" else "bitwarden.eu"
                            "${accountHost(ref)} did not accept the password for ${ref.email}. If the account is on $other, add it again with that choice."
                        } else {
                            failure.message?.let { it.replaceFirstChar(Char::uppercase) + "." } ?: "Wrong password."
                        }
                        retryAt = Pass.noteWrongAccountPassword()
                        delay(900)
                        shield = ShieldState.IDLE
                    }
                }
            } finally {
                password.fill(0)
            }
        }
    }

    fun submit(factors: List<Pair<String, ByteArray>> = emptyList()) {
        source?.let {
            submitAccount(it)
            return
        }
        if (shield == ShieldState.WORKING || field.text.isEmpty() || remaining > 0) return
        val password = field.text.toString().toByteArray()
        shield = ShieldState.WORKING
        message = null
        scope.launch {
            val result = runCatching { Pass.unlock(password, factors) }
            password.fill(0)
            factors.forEach { it.second.fill(0) }
            when (val outcome = result.getOrNull()) {
                UnlockResult.Opened -> {
                    needed = null
                    field.clearText()
                    keyboard?.hide()
                    haptics.confirm()
                    shield = ShieldState.OPENED
                }
                is UnlockResult.Wrong -> {
                    haptics.reject()
                    shield = ShieldState.REFUSED
                    message = "Wrong password."
                    retryAt = outcome.retryAtMs
                    delay(900)
                    shield = ShieldState.IDLE
                }
                is UnlockResult.Waiting -> {
                    haptics.reject()
                    shield = ShieldState.IDLE
                    retryAt = outcome.retryAtMs
                }
                is UnlockResult.NeedsFactor -> {
                    shield = ShieldState.IDLE
                    needed = outcome.factors
                }
                UnlockResult.HardLocked -> {
                    field.clearText()
                    shield = ShieldState.REFUSED
                    message = "Wrong password."
                    retryAt = Pass.retryAt()
                }
                null -> {
                    shield = ShieldState.IDLE
                    message = result.exceptionOrNull()?.message ?: "The vault could not be read."
                }
            }
        }
    }

    fun promptBiometric() {
        val host = activity ?: return
        val unlocker = quick ?: return
        val cipher = runCatching { unlocker.cipherForUnlocking() }.getOrNull() ?: return
        val prompt = BiometricPrompt(
            host,
            ContextCompat.getMainExecutor(host),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    val authorised = result.cryptoObject?.cipher ?: return
                    shield = ShieldState.WORKING
                    scope.launch {
                        val key = unlocker.unwrap(authorised)
                        val file = unlocker.enrolledFile()
                        val done = runCatching { Pass.unlockWithKey(key, file!!) }
                        key.fill(0)
                        if (done.isSuccess) {
                            haptics.confirm()
                            shield = ShieldState.OPENED
                        } else {
                            shield = ShieldState.IDLE
                            message = "Fingerprint unlock failed. Enter your password."
                        }
                    }
                }
            },
        )
        prompt.authenticate(
            BiometricPrompt.PromptInfo.Builder()
                .setTitle("Unlock WM Pass")
                .setNegativeButtonText("Use password")
                .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                .build(),
            BiometricPrompt.CryptoObject(cipher),
        )
    }

    LaunchedEffect(Unit) {
        if (biometricsReady && source == null) {
            delay(240)
            promptBiometric()
        } else if (remaining == 0L) {
            delay(420)
            runCatching { focus.requestFocus() }
        }
    }

    if (colors.flat) {
        val busy = shield == ShieldState.WORKING
        Column(
            Modifier
                .fillMaxSize()
                .background(colors.ground)
                .statusBarsPadding()
                .imePadding()
                .navigationBarsPadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp),
        ) {
            Spacer(Modifier.weight(1f))
            // The mark's box has margin in it; pull it back so the shield lines up with the text.
            ShieldMark(shield, Modifier.offset(x = (-17).dp).size(72.dp), 0)
            Spacer(Modifier.height(20.dp))
            Text(source?.label ?: "WM Pass", color = colors.text, style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                when {
                    remaining > 0 -> "Too many attempts. Try again in ${formatWait(remaining)}."
                    busy -> "Unlocking…"
                    else -> "Enter your password to unlock."
                },
                color = if (remaining > 0) colors.danger else colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            if (notice != null) {
                Spacer(Modifier.height(12.dp))
                Text(notice, color = colors.text, style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(24.dp))
            if (accounts.isNotEmpty()) {
                FlowRow(Modifier.fillMaxWidth().padding(bottom = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (hasLocal) {
                        Pill("This device", source == null) {
                            source = null
                            message = null
                            needed = null
                        }
                    } else {
                        Pill("New vault", false, icon = PassIcons.Add, onClick = onStartLocal)
                    }
                    accounts.forEach { account ->
                        Pill(account.label, source?.id == account.id) {
                            source = account
                            message = null
                            needed = null
                            asksCode = false
                            keyFile = null
                        }
                    }
                }
            }
            needed?.let { factors ->
                FactorEntry(factors, enabled = !busy) { kind, output -> submit(listOf(kind to output)) }
            }
            AnimatedVisibility(asksCode) {
                Box(Modifier.padding(bottom = 10.dp)) { com.wasimaster.wmpass.ui.vault.SheetField(codeField, if (emailedCode) "Code from the email" else "Two-factor code") }
            }
            PasswordPill(
                field,
                source?.let { "Password" } ?: "Master password",
                enabled = remaining == 0L && !busy,
                imeAction = ImeAction.Go,
                focus = focus,
                error = message != null,
                onSubmit = { submit() },
            )
            AnimatedVisibility(message != null, enter = fadeIn(), exit = fadeOut()) {
                Text(message.orEmpty(), color = colors.danger, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 16.dp, top = 4.dp))
            }
            Spacer(Modifier.height(16.dp))
            PrimaryButton(if (busy) "Unlocking…" else "Unlock", Modifier.fillMaxWidth(), enabled = field.text.isNotEmpty() && remaining == 0L && !busy) { submit() }
            if (source?.kind == "kdbx") {
                Spacer(Modifier.height(8.dp))
                androidx.compose.material3.TextButton(onClick = { pickAccountKey.launch(arrayOf("*/*")) }, modifier = Modifier.fillMaxWidth()) {
                    Text(if (keyFile == null) "Add key file" else "Key file added")
                }
            }
            if (biometricsReady && source == null) {
                Spacer(Modifier.height(8.dp))
                androidx.compose.material3.OutlinedButton(onClick = { promptBiometric() }, modifier = Modifier.fillMaxWidth().height(48.dp)) {
                    Icon(PassIcons.Fingerprint, null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Use fingerprint")
                }
            }
            Spacer(Modifier.weight(1.4f))
        }
        return
    }
    Column(Modifier.fillMaxSize().statusBarsPadding().imePadding().navigationBarsPadding()) {
        ShieldSlot(shield, typed, Modifier.weight(1f))
        Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 16.dp)) {
            Text(
                "Welcome back",
                color = colors.text,
                style = MaterialTheme.typography.headlineMedium,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(bottom = 18.dp),
            )
            AnimatedVisibility(
                visible = message != null || remaining > 0,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                Text(
                    if (remaining > 0) "Too many tries. Next one in ${formatWait(remaining)}." else message.orEmpty(),
                    color = colors.danger,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(bottom = 12.dp),
                )
            }
            if (accounts.isNotEmpty()) {
                FlowRow(
                    Modifier.fillMaxWidth().padding(bottom = 14.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (hasLocal) {
                        Pill("This phone", source == null) {
                            source = null
                            message = null
                            needed = null
                        }
                    } else {
                        Pill("Make a vault here", false, icon = PassIcons.Add, onClick = onStartLocal)
                    }
                    accounts.forEach { account ->
                        Pill(account.label, source?.id == account.id) {
                            source = account
                            message = null
                            needed = null
                            asksCode = false
                            keyFile = null
                        }
                    }
                }
            }
            val factors = needed
            if (factors != null) {
                FactorEntry(factors, enabled = shield != ShieldState.WORKING) { kind, output -> submit(listOf(kind to output)) }
            }
            AnimatedVisibility(asksCode) {
                Box(Modifier.padding(bottom = 10.dp)) { com.wasimaster.wmpass.ui.vault.SheetField(codeField, if (emailedCode) "Code from the email" else "Six-digit code") }
            }
            PasswordPill(
                field,
                source?.let { "Password for ${it.label}" } ?: "Master password",
                enabled = remaining == 0L && shield != ShieldState.WORKING,
                imeAction = ImeAction.Go,
                focus = focus,
                onSubmit = { submit() },
            ) {
                GoButton(field.text.isNotEmpty() && remaining == 0L && shield != ShieldState.WORKING) { submit() }
            }
            if (source?.kind == "kdbx") {
                Spacer(Modifier.height(12.dp))
                QuietButton(
                    if (keyFile == null) "Add its key file, if it has one" else "Key file chosen",
                    Modifier.fillMaxWidth(),
                    PassIcons.Key,
                    colors.primary,
                    height = 52.dp,
                ) { pickAccountKey.launch(arrayOf("*/*")) }
            }
            if (biometricsReady && source == null) {
                Spacer(Modifier.height(12.dp))
                QuietButton(
                    "Use fingerprint",
                    Modifier.fillMaxWidth(),
                    PassIcons.Fingerprint,
                    colors.primary,
                    height = 52.dp,
                ) { promptBiometric() }
            }
        }
    }
}

/**
 * The second thing a vault asks for, once the password has gone through. A key file is chosen; a
 * hardware key answers as soon as it is held to the phone or plugged in.
 */
@Composable
private fun FactorEntry(factors: List<NeededFactor>, enabled: Boolean, onOutput: (String, ByteArray) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val activity = context as? FragmentActivity
    val hardware = factors.filter { it.kind != "key_file" }
    var problem by remember { mutableStateOf<String?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) context.contentResolver.openInputStream(uri)?.use { it.readBytes() }?.let { onOutput("key_file", it) }
    }
    if (hardware.isNotEmpty() && activity != null) {
        LaunchedEffect(factors, enabled) {
            if (!enabled) return@LaunchedEffect
            var index = 0
            while (true) {
                val factor = hardware[index % hardware.size]
                val json = buildJsonObject {
                    put("kind", factor.kind)
                    factor.credentialId?.let { put("credential_id", it) }
                    factor.salt?.let { put("salt", it) }
                    factor.challenge?.let { put("challenge", it) }
                    factor.slot?.let { put("slot", it) }
                }.toString()
                try {
                    val (_, output) = HardwareKeys.tap(activity, factor.kind == "fido2", json)
                    onOutput(factor.kind, output)
                    return@LaunchedEffect
                } catch (failure: NativeFailure) {
                    problem = if (hardware.size > 1) "That key did not answer. Trying the next one." else failure.message
                    index++
                    delay(1200)
                }
            }
        }
        Column(Modifier.fillMaxWidth().padding(bottom = 12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            com.wasimaster.wmpass.ui.parts.Listening(PassIcons.SecurityKey, size = 128.dp)
            Spacer(Modifier.height(8.dp))
            Text(
                problem ?: "Hold your key to the back of the phone, or plug it in and touch it.",
                color = if (problem != null) colors.danger else colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                textAlign = TextAlign.Center,
            )
        }
    }
    if (factors.any { it.kind == "key_file" }) {
        Text(
            "This vault also needs its key file.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(bottom = 10.dp),
        )
        QuietButton("Choose the key file", Modifier.fillMaxWidth().padding(bottom = 12.dp), PassIcons.Key, colors.primary, height = 52.dp) {
            pick.launch(arrayOf("*/*"))
        }
    }
}

// ---- the first visit ----

private enum class Step { HELLO, CHOOSE, CONFIRM, MAKING }

/** First run in the standard look: one form, with everything it needs on one page. */
@Composable
private fun StandardWelcome(onBack: (() -> Unit)?) {
    val colors = palette
    val scope = rememberCoroutineScope()
    val haptics = rememberHaptics()
    val first = rememberTextFieldState()
    val second = rememberTextFieldState()
    var strength by remember { mutableStateOf(StrengthView()) }
    var making by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var returning by remember { mutableStateOf(false) }
    BackHandler(onBack != null) { onBack?.invoke() }
    LaunchedEffect(first) {
        snapshotFlow { first.text.toString() }.collectLatest { text ->
            delay(120)
            strength = if (text.isNotEmpty()) Pass.strength(text.toByteArray()) else StrengthView()
        }
    }
    val matches = second.text.toString() == first.text.toString()
    val ready = first.text.isNotEmpty() && matches && strength.score >= 3 && !making
    fun make() {
        making = true
        error = null
        val password = first.text.toString().toByteArray()
        scope.launch {
            val result = runCatching { Pass.create("Personal", password) }
            password.fill(0)
            result.onFailure {
                error = it.message
                making = false
                haptics.reject()
            }.onSuccess {
                first.clearText()
                second.clearText()
                haptics.confirm()
            }
        }
    }
    Column(
        Modifier
            .fillMaxSize()
            .background(colors.ground)
            .statusBarsPadding()
            .imePadding()
            .navigationBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 24.dp),
    ) {
        ShieldMark(ShieldState.IDLE, Modifier.offset(x = (-15).dp).size(64.dp), 0)
        Spacer(Modifier.height(20.dp))
        Text("Create your vault", color = colors.text, style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.height(8.dp))
        Text(
            "Your master password encrypts everything on this device. It cannot be reset, so keep a copy somewhere safe.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
        )
        MasterPasswordLink(Modifier.offset(x = (-12).dp))
        Spacer(Modifier.height(12.dp))
        PasswordPill(first, "Master password", imeAction = ImeAction.Next, enabled = !making)
        Spacer(Modifier.height(8.dp))
        StrengthBar(strength.score, Modifier.fillMaxWidth(), active = first.text.isNotEmpty())
        Spacer(Modifier.height(6.dp))
        Text(
            when {
                first.text.isEmpty() -> "Four or five random words work well."
                strength.warning != null -> strength.warning!!
                else -> "Time to crack: ${strength.crackSlowHash}"
            },
            color = colors.textMid,
            style = MaterialTheme.typography.bodySmall,
        )
        androidx.compose.material3.TextButton(
            onClick = {
                scope.launch {
                    val reply = Engine.call(
                        "gen.generate",
                        args = {
                            putJsonObject("Passphrase") {
                                put("words", 5)
                                put("separator", "-")
                                put("capitalize", false)
                                put("include_digit", false)
                            }
                        },
                    )
                    val words = reply.secret!!.consumeText()
                    first.edit { replace(0, length, words) }
                    second.clearText()
                    haptics.confirm()
                }
            },
            enabled = !making,
        ) { Text("Generate a passphrase") }
        Spacer(Modifier.height(8.dp))
        PasswordPill(second, "Confirm master password", enabled = !making, onSubmit = { if (ready) make() })
        if (second.text.isNotEmpty() && !matches) {
            Text("Passwords do not match.", color = colors.danger, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 16.dp, top = 4.dp))
        }
        error?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 16.dp, top = 4.dp)) }
        Spacer(Modifier.height(20.dp))
        PrimaryButton(if (making) "Creating vault…" else "Create vault", Modifier.fillMaxWidth(), enabled = ready) { make() }
        Spacer(Modifier.height(8.dp))
        androidx.compose.material3.TextButton(onClick = { returning = true }, modifier = Modifier.fillMaxWidth()) { Text("I already have a vault") }
    }
    if (returning) ComingBack { returning = false }
}

@Composable
private fun Welcome(onBack: (() -> Unit)?) {
    if (palette.flat) {
        StandardWelcome(onBack)
        return
    }
    var step by rememberSaveable { mutableStateOf(Step.HELLO) }
    val first = rememberTextFieldState()
    val second = rememberTextFieldState()
    val scope = rememberCoroutineScope()
    val haptics = rememberHaptics()
    var error by remember { mutableStateOf<String?>(null) }
    BackHandler(step == Step.CHOOSE || step == Step.CONFIRM) {
        step = if (step == Step.CONFIRM) Step.CHOOSE else Step.HELLO
    }
    BackHandler(step == Step.HELLO && onBack != null) { onBack?.invoke() }

    fun make() {
        step = Step.MAKING
        val password = first.text.toString().toByteArray()
        scope.launch {
            val result = runCatching { Pass.create("Personal", password) }
            password.fill(0)
            result.onFailure {
                error = it.message
                step = Step.CHOOSE
                haptics.reject()
            }.onSuccess {
                first.clearText()
                second.clearText()
                haptics.confirm()
            }
        }
    }

    Column(Modifier.fillMaxSize().statusBarsPadding().imePadding().navigationBarsPadding()) {
        val shield = if (step == Step.MAKING) ShieldState.WORKING else ShieldState.IDLE
        ShieldSlot(shield, modifier = Modifier.weight(1f))
        AnimatedContent(
            targetState = step,
            transitionSpec = {
                val forward = targetState.ordinal > initialState.ordinal
                (slideInHorizontally(tween(360)) { if (forward) it / 4 else -it / 4 } + fadeIn(tween(240))) togetherWith
                    (slideOutHorizontally(tween(300)) { if (forward) -it / 4 else it / 4 } + fadeOut(tween(160)))
            },
            label = "welcome",
        ) { current ->
            Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp).padding(bottom = 20.dp)) {
                when (current) {
                    Step.HELLO -> Hello(onCreate = { step = Step.CHOOSE })
                    Step.CHOOSE -> Choose(first, error) {
                        error = null
                        step = Step.CONFIRM
                    }
                    Step.CONFIRM -> Confirm(first, second, onBack = { step = Step.CHOOSE }, onDone = ::make)
                    Step.MAKING -> Making()
                }
            }
        }
    }
}

@Composable
private fun Headline(title: String, body: String) {
    val colors = palette
    Text(
        title,
        color = colors.text,
        style = MaterialTheme.typography.headlineLarge.copy(fontSize = 31.sp, lineHeight = 37.sp, lineBreak = LineBreak.Heading),
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth(),
    )
    Spacer(Modifier.height(10.dp))
    Text(
        body,
        color = colors.textMid,
        style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Normal, lineHeight = 23.sp, lineBreak = LineBreak.Paragraph),
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
    )
}

@Composable
private fun Hello(onCreate: () -> Unit) {
    var returning by remember { mutableStateOf(false) }
    Headline("A vault only you can open", "Your passwords stay on this phone, encrypted, and nothing leaves unless you send it.")
    Spacer(Modifier.height(28.dp))
    PrimaryButton("Create your vault", Modifier.fillMaxWidth(), onClick = onCreate)
    Spacer(Modifier.height(10.dp))
    QuietButton("I already have one", Modifier.fillMaxWidth()) { returning = true }
    MasterPasswordLink(Modifier.fillMaxWidth().padding(top = 4.dp))
    if (returning) ComingBack { returning = false }
}

@Composable
private fun DoorRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, subtitle: String, onClick: () -> Unit) {
    val colors = palette
    val haptics = rememberHaptics()
    Row(
        Modifier
            .fillMaxWidth()
            .clip(com.wasimaster.wmpass.ui.theme.Shapes.cardSmall)
            .clickable {
                haptics.tick()
                onClick()
            }
            .padding(vertical = 12.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        com.wasimaster.wmpass.ui.parts.GlyphTile(icon, colors.primary, size = 44.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
            Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
        }
    }
}

/** Every way back to a vault that already exists somewhere. */
@Composable
private fun ComingBack(onDismiss: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var sheet by remember { mutableStateOf("menu") }
    var problem by remember { mutableStateOf<String?>(null) }
    var place by remember { mutableStateOf<Destination?>(null) }
    val openFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            scope.launch {
                try {
                    val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
                    Pass.restore(bytes)
                    onDismiss()
                } catch (failure: Exception) {
                    problem = failure.message ?: "That file could not be used."
                }
            }
        }
    }
    when (sheet) {
        "menu" -> PassSheet(onDismiss, "Existing vault") {
            if (!com.wasimaster.wmpass.BuildConfig.OFFLINE) {
                DoorRow(PassIcons.Devices, "Get it from another device", "Your other phone or computer sends it over Wi-Fi") { sheet = "join" }
            }
            DoorRow(PassIcons.Folder, "Restore from a file", ".wmvault or .wmbackup") { openFile.launch(arrayOf("*/*")) }
            DoorRow(PassIcons.Backup, "Restore from a backup location", "Folder, WebDAV, S3 or SFTP") { sheet = "place" }
            DoorRow(PassIcons.Sync, "Open a KeePass file or sign in", "KeePass, Bitwarden, Vaultwarden or a WM Pass server") { sheet = "account" }
            DoorRow(PassIcons.FamilyRestroom, "Recover with emergency shares", "When someone gave you part of their key") { sheet = "shares" }
            problem?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
            }
        }
        "place" -> com.wasimaster.wmpass.ui.settings.DestinationSheet(null, onDismiss) { saved ->
            place = saved
            sheet = "copies"
        }
        "copies" -> place?.let { saved ->
            com.wasimaster.wmpass.ui.settings.CopiesSheet(saved, onDismiss) { bytes ->
                runCatching { Pass.restore(bytes) }
                    .onSuccess { onDismiss() }
                    .onFailure {
                        problem = it.message
                        sheet = "menu"
                    }
            }
        }
        "account" -> com.wasimaster.wmpass.ui.settings.AddAccountSheet(onDismiss)
        "shares" -> SharesSheet(onDismiss)
        "join" -> JoinSheet(onDismiss)
    }
}

/** A trustee's way in: their shares, a copy of the vault, and a new password to replace the lost one. */
@Composable
private fun SharesSheet(onDismiss: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val shares = rememberTextFieldState()
    val password = rememberTextFieldState()
    var file by remember { mutableStateOf<ByteArray?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) file = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
    }
    PassSheet(onDismiss, "Emergency shares") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                "The kit lists how many shares are needed and the agreed waiting period. WM Pass cannot enforce the wait.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            QuietButton(if (file == null) "Choose a copy of the vault" else "Vault copy chosen", Modifier.fillMaxWidth(), PassIcons.Folder, colors.primary) { pick.launch(arrayOf("*/*")) }
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(130.dp)
                    .clip(com.wasimaster.wmpass.ui.theme.Shapes.cardSmall)
                    .background(colors.surface2)
                    .padding(16.dp),
            ) {
                if (shares.text.isEmpty()) Text("Paste the shares, one on each line", color = colors.textLow, style = MaterialTheme.typography.bodyLarge)
                androidx.compose.foundation.text.BasicTextField(
                    shares,
                    Modifier.fillMaxWidth(),
                    textStyle = PassType.secretSmall.copy(color = colors.text),
                    cursorBrush = SolidColor(colors.primary),
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Password),
                )
            }
            PasswordPill(password, "A new password for it")
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            PrimaryButton(if (busy) "Opening…" else "Open it", Modifier.fillMaxWidth(), enabled = !busy && file != null && shares.text.isNotBlank() && password.text.isNotEmpty()) {
                busy = true
                scope.launch {
                    val secret = password.text.toString().toByteArray()
                    try {
                        Pass.recover(file!!, shares.text.lines().map { it.trim() }.filter { it.isNotEmpty() }, secret)
                        onDismiss()
                    } catch (failure: NativeFailure) {
                        problem = failure.message
                    } finally {
                        secret.fill(0)
                        busy = false
                    }
                }
            }
        }
    }
}

@Composable
private fun Choose(field: TextFieldState, error: String?, onNext: () -> Unit) {
    val colors = palette
    val scope = rememberCoroutineScope()
    val haptics = rememberHaptics()
    var strength by remember { mutableStateOf(StrengthView()) }
    var suggestion by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(field) {
        snapshotFlow { field.text.toString() }.collectLatest { text ->
            delay(120)
            if (text.isNotEmpty()) strength = Pass.strength(text.toByteArray()) else strength = StrengthView()
        }
    }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) {
        delay(360)
        runCatching { focus.requestFocus() }
    }
    Headline("Choose your master password", "It is the one thing nobody can reset for you, us included. Keep a copy somewhere safe.")
    Spacer(Modifier.height(20.dp))
    PasswordPill(field, "Master password", imeAction = ImeAction.Next, focus = focus, onSubmit = {
        if (strength.score >= 3) onNext()
    })
    Spacer(Modifier.height(12.dp))
    StrengthBar(strength.score, Modifier.fillMaxWidth().padding(horizontal = 16.dp), active = field.text.isNotEmpty())
    Spacer(Modifier.height(8.dp))
    val verdict = when {
        error != null -> error
        field.text.isEmpty() -> "Four random words beat any clever password."
        strength.warning != null -> strength.warning!!
        else -> "Would take ${strength.crackSlowHash} to crack."
    }
    AnimatedContent(verdict, label = "verdict", transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(120)) }) { line ->
        Text(
            line,
            color = if (strength.score >= 3) colors.primary else colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
    }
    val shown = suggestion
    if (shown != null) {
        Spacer(Modifier.height(10.dp))
        SecretText(shown, Modifier.fillMaxWidth().padding(horizontal = 16.dp), PassType.secretSmall)
    }
    Spacer(Modifier.height(18.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        QuietButton("Suggest one", Modifier.weight(1f), PassIcons.Casino, colors.primary) {
            scope.launch {
                val reply = Engine.call(
                    "gen.generate",
                    args = {
                        putJsonObject("Passphrase") {
                            put("words", 5)
                            put("separator", "-")
                            put("capitalize", false)
                            put("include_digit", false)
                        }
                    },
                )
                val words = reply.secret!!.consumeText()
                suggestion = words
                field.edit { replace(0, length, words) }
                haptics.confirm()
            }
        }
        PrimaryButton("Continue", Modifier.weight(1f), enabled = strength.score >= 3 && field.text.isNotEmpty(), onClick = onNext)
    }
}

@Composable
private fun Confirm(first: TextFieldState, second: TextFieldState, onBack: () -> Unit, onDone: () -> Unit) {
    val colors = palette
    val matches = second.text.toString() == first.text.toString()
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) {
        delay(360)
        runCatching { focus.requestFocus() }
    }
    Headline("Once more, from memory", "If you cannot type it now, you will not be able to next week either.")
    Spacer(Modifier.height(20.dp))
    PasswordPill(second, "Master password again", imeAction = ImeAction.Done, focus = focus, onSubmit = {
        if (matches) onDone()
    })
    Spacer(Modifier.height(10.dp))
    Text(
        if (second.text.isEmpty()) " " else if (matches) "They match." else "Not the same yet.",
        color = if (matches) colors.primary else colors.textMid,
        style = MaterialTheme.typography.bodyMedium,
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth(),
    )
    Spacer(Modifier.height(16.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        QuietButton("Back", Modifier.width(110.dp), onClick = onBack)
        PrimaryButton("Lock it in", Modifier.weight(1f), enabled = matches && second.text.isNotEmpty(), onClick = onDone)
    }
}

@Composable
private fun Making() {
    Headline("Forging your key", "Deliberately slow, so that guessing your password is too.")
    Spacer(Modifier.height(90.dp))
}

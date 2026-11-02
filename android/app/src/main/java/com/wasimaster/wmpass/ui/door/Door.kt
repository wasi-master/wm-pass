package com.wasimaster.wmpass.ui.door

import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicSecureTextField
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.LineBreak
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.NeededFactor
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.QuickUnlock
import com.wasimaster.wmpass.core.StrengthView
import com.wasimaster.wmpass.core.UnlockResult
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
import com.wasimaster.wmpass.ui.theme.MonoFamily
import com.wasimaster.wmpass.ui.theme.PassType
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
    Box(Modifier.fillMaxSize()) {
        Aurora(Modifier.fillMaxSize())
        when (state) {
            LockState.NoVault -> Welcome()
            else -> Unlock()
        }
    }
}

@Composable
private fun ShieldSlot(state: ShieldState, typed: Int = 0, modifier: Modifier = Modifier) {
    // Centred on the same point every time, because the opening starts from it.
    Box(modifier.fillMaxWidth().height((DoorShieldCentreDp * 2).dp.coerceAtMost(420.dp)), contentAlignment = Alignment.TopCenter) {
        ShieldMark(
            state,
            Modifier
                .padding(top = (DoorShieldCentreDp - 96).dp)
                .size(176.dp),
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
    onSubmit: () -> Unit = {},
    trailing: @Composable (() -> Unit)? = null,
) {
    val colors = palette
    var showing by remember { mutableStateOf(false) }
    val haptics = rememberHaptics()
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
                    .then(if (focus != null) Modifier.focusRequester(focus) else Modifier),
                textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 17.sp, color = colors.text),
                cursorBrush = SolidColor(colors.primary),
                textObfuscationMode = if (showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Password,
                    autoCorrectEnabled = false,
                    imeAction = imeAction,
                ),
                onKeyboardAction = KeyboardActionHandler { onSubmit() },
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
            tint = if (enabled) androidx.compose.ui.graphics.Color.White else colors.textLow,
            modifier = Modifier.size(24.dp),
        )
    }
}

// ---- every visit after the first ----

private fun formatWait(ms: Long): String {
    val seconds = (ms / 1000).coerceAtLeast(1)
    return if (seconds >= 60) "${seconds / 60} min ${seconds % 60} s" else "$seconds s"
}

@Composable
private fun Unlock() {
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

    fun submit(factors: List<Pair<String, ByteArray>> = emptyList()) {
        if (shield == ShieldState.WORKING || field.text.isEmpty() || remaining > 0) return
        val password = field.text.toString().toByteArray()
        shield = ShieldState.WORKING
        message = null
        scope.launch {
            val result = runCatching { Pass.unlock(password, factors) }
            password.fill(0)
            when (val outcome = result.getOrNull()) {
                UnlockResult.Opened -> {
                    field.clearText()
                    keyboard?.hide()
                    haptics.confirm()
                    shield = ShieldState.OPENED
                }
                is UnlockResult.Wrong -> {
                    haptics.reject()
                    shield = ShieldState.REFUSED
                    message = "That is not it."
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
                    message = "That is not it."
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
                            message = "Fingerprint unlock could not open this vault. Use the password."
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
        if (biometricsReady) {
            delay(240)
            promptBiometric()
        } else if (remaining == 0L) {
            delay(420)
            runCatching { focus.requestFocus() }
        }
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
            val factors = needed
            if (factors != null) {
                Text(
                    factorPrompt(factors),
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(bottom = 12.dp),
                )
            }
            PasswordPill(
                field,
                "Master password",
                enabled = remaining == 0L && shield != ShieldState.WORKING,
                imeAction = ImeAction.Go,
                focus = focus,
                onSubmit = { submit() },
            ) {
                GoButton(field.text.isNotEmpty() && remaining == 0L && shield != ShieldState.WORKING) { submit() }
            }
            if (biometricsReady) {
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

private fun factorPrompt(factors: List<NeededFactor>): String = when (factors.firstOrNull()?.kind) {
    "key_file" -> "This vault also needs its key file."
    "fido2" -> "Touch your security key."
    "yubikey" -> "Touch your YubiKey."
    else -> "This vault needs a second factor."
}

// ---- the first visit ----

private enum class Step { HELLO, CHOOSE, CONFIRM, MAKING }

@Composable
private fun Welcome() {
    var step by rememberSaveable { mutableStateOf(Step.HELLO) }
    val first = rememberTextFieldState()
    val second = rememberTextFieldState()
    val scope = rememberCoroutineScope()
    val haptics = rememberHaptics()
    var error by remember { mutableStateOf<String?>(null) }
    BackHandler(step == Step.CHOOSE || step == Step.CONFIRM) {
        step = if (step == Step.CONFIRM) Step.CHOOSE else Step.HELLO
    }

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
    Headline("A vault only you can open", "Your passwords stay on this phone, encrypted, and nothing leaves unless you send it.")
    Spacer(Modifier.height(28.dp))
    PrimaryButton("Create your vault", Modifier.fillMaxWidth(), onClick = onCreate)
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

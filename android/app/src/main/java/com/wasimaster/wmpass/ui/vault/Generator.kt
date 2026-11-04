package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.PresetView
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.core.StrengthView
import com.wasimaster.wmpass.ui.fx.Aurora
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.SecretText
import com.wasimaster.wmpass.ui.parts.StrengthBar
import com.wasimaster.wmpass.ui.parts.bleed
import com.wasimaster.wmpass.ui.parts.fadeEdges
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

private const val BUFFER = 64

/**
 * The generator's settings and its current value.
 *
 * Passwords are drawn once at the longest length and shown as a prefix, so dragging the length
 * adds and removes one character at a time instead of replacing them all. A prefix can miss a
 * class the longer value had, so letting go checks for that and draws again if it did, which is
 * the same rejection any generator uses to guarantee a class.
 */
@Stable
class GenState {
    companion object {
        /** A generator set up the way the owner chose in settings. */
        fun fromPrefs(prefs: com.wasimaster.wmpass.app.Prefs) = GenState().apply {
            mode = prefs.genMode.value
            length = prefs.genLength.value.coerceIn(8, BUFFER)
            words = prefs.genWords.value
            symbols = prefs.genSymbols.value
            noAmbiguous = prefs.genNoAmbiguous.value
        }
    }

    var mode by mutableStateOf("password")
    var length by mutableIntStateOf(20)
    var lower by mutableStateOf(true)
    var upper by mutableStateOf(true)
    var digits by mutableStateOf(true)
    var symbols by mutableStateOf(true)
    var noAmbiguous by mutableStateOf(false)
    var words by mutableIntStateOf(5)
    var separator by mutableStateOf("-")
    var capitalize by mutableStateOf(false)
    var includeDigit by mutableStateOf(true)
    var syllables by mutableIntStateOf(6)
    var pin by mutableIntStateOf(6)
    var pattern by mutableStateOf("Cvcv-####")

    private var buffer by mutableStateOf("")
    private var drawnBits by mutableStateOf(0.0)
    var whole by mutableStateOf("")
        private set

    val value: String
        get() = if (mode == "password") buffer.take(length) else whole

    val bits: Double
        get() = if (mode == "password") drawnBits * length / BUFFER else drawnBits

    private fun recipe(): JsonObjectBuilder.() -> Unit = {
        when (mode) {
            "password" -> put(
                "Password",
                buildJsonObject {
                    put("length", BUFFER)
                    put("lower", lower)
                    put("upper", upper)
                    put("digits", digits)
                    put("symbols", symbols)
                    put("exclude_ambiguous", noAmbiguous)
                },
            )
            "passphrase" -> put(
                "Passphrase",
                buildJsonObject {
                    put("words", words)
                    put("separator", separator)
                    put("capitalize", capitalize)
                    put("include_digit", includeDigit)
                },
            )
            "pronounceable" -> put(
                "Pronounceable",
                buildJsonObject {
                    put("syllables", syllables)
                    put("separator", "")
                },
            )
            "pin" -> put("Pin", buildJsonObject { put("length", pin) })
            else -> put("Pattern", pattern)
        }
    }

    private fun prefixIsWhole(candidate: String): Boolean {
        val shown = candidate.take(length)
        if (lower && shown.none { it.isLowerCase() }) return false
        if (upper && shown.none { it.isUpperCase() }) return false
        if (digits && shown.none { it.isDigit() }) return false
        if (symbols && shown.all { it.isLetterOrDigit() }) return false
        return true
    }

    suspend fun roll() {
        if (mode == "password" && !lower && !upper && !digits && !symbols) lower = true
        repeat(40) {
            val reply = Engine.call("gen.generate", args = recipe())
            val text = reply.secret?.consumeText().orEmpty()
            drawnBits = reply.obj["bits"]?.jsonPrimitive?.content?.toDoubleOrNull() ?: 0.0
            if (mode == "password") {
                buffer = text
                if (prefixIsWhole(text)) return
            } else {
                whole = text
                return
            }
        }
    }

    /** After a drag: keep the prefix if it still holds every class asked for. */
    suspend fun settle() {
        if (mode == "password" && !prefixIsWhole(buffer)) roll()
    }

    fun apply(preset: PresetView) {
        val obj = preset.recipe as? JsonObject
        val string = preset.recipe.let { (it as? kotlinx.serialization.json.JsonPrimitive)?.content }
        fun JsonObject.flag(key: String, default: Boolean) = this[key]?.jsonPrimitive?.booleanOrNull ?: default
        fun JsonObject.int(key: String, default: Int) = this[key]?.jsonPrimitive?.intOrNull ?: default
        when {
            obj?.containsKey("Password") == true -> {
                val p = obj["Password"]!!.jsonObject
                mode = "password"
                length = p.int("length", 20).coerceIn(8, BUFFER)
                lower = p.flag("lower", true)
                upper = p.flag("upper", true)
                digits = p.flag("digits", true)
                symbols = p.flag("symbols", true)
                noAmbiguous = p.flag("exclude_ambiguous", false)
            }
            obj?.containsKey("Passphrase") == true -> {
                val p = obj["Passphrase"]!!.jsonObject
                mode = "passphrase"
                words = p.int("words", 5)
                separator = p["separator"]?.jsonPrimitive?.content ?: "-"
                capitalize = p.flag("capitalize", false)
                includeDigit = p.flag("include_digit", true)
            }
            obj?.containsKey("Pronounceable") == true -> {
                mode = "pronounceable"
                syllables = obj["Pronounceable"]!!.jsonObject.int("syllables", 6)
            }
            obj?.containsKey("Pin") == true -> {
                mode = "pin"
                pin = obj["Pin"]!!.jsonObject.int("length", 6)
            }
            string != null -> mode = "password"
        }
    }
}

private val Modes = listOf(
    "password" to "Random",
    "passphrase" to "Words",
    "pronounceable" to "Pronounceable",
    "pin" to "PIN",
    "pattern" to "Pattern",
)

/**
 * The value itself, set large. It is the whole point of the page, so it gets the most room,
 * and a downward pull anywhere on it rolls a new one.
 */
@Composable
fun GeneratedValue(state: GenState, modifier: Modifier = Modifier, large: Boolean = true) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val pull = remember { Animatable(0f) }
    val threshold = with(LocalDensity.current) { 72.dp.toPx() }
    var armed by remember { mutableStateOf(false) }
    if (colors.flat) {
        androidx.compose.material3.Surface(
            modifier.fillMaxWidth().heightIn(min = 112.dp),
            color = colors.surface1,
            shape = MaterialTheme.shapes.medium,
        ) {
            // Generating again belongs next to what it replaces, and its one turn says it did.
            var rolls by remember { mutableStateOf(0) }
            val spin by androidx.compose.animation.core.animateFloatAsState(rolls * 360f, androidx.compose.animation.core.tween(420), label = "spin")
            Row(Modifier.padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) {
                    SecretText(state.value.ifEmpty { " " }, style = PassType.secret.copy(fontSize = 20.sp, lineHeight = 28.sp))
                }
                androidx.compose.material3.IconButton(onClick = {
                    rolls++
                    shell.haptics.click()
                    scope.launch { state.roll() }
                }) {
                    Icon(PassIcons.Refresh, "Generate again", Modifier.graphicsLayer { rotationZ = spin })
                }
            }
        }
        return
    }
    Box(
        modifier
            .fillMaxWidth()
            .heightIn(min = if (large) 168.dp else 120.dp)
            .pointerInput(Unit) {
                detectVerticalDragGestures(
                    onDragEnd = {
                        val fire = armed
                        armed = false
                        scope.launch { pull.animateTo(0f, Springs.lively()) }
                        if (fire) {
                            shell.haptics.confirm()
                            scope.launch { state.roll() }
                        }
                    },
                    onDragCancel = {
                        armed = false
                        scope.launch { pull.animateTo(0f, Springs.lively()) }
                    },
                ) { change, drag ->
                    change.consume()
                    scope.launch { pull.snapTo((pull.value + drag * 0.5f).coerceIn(0f, threshold * 1.4f)) }
                    val now = pull.value >= threshold
                    if (now != armed) {
                        armed = now
                        shell.haptics.tick()
                    }
                }
            }
            .graphicsLayer { translationY = pull.value },
        contentAlignment = Alignment.Center,
    ) {
        val size = when {
            state.value.length <= 16 -> if (large) 34.sp else 26.sp
            state.value.length <= 28 -> if (large) 27.sp else 22.sp
            state.value.length <= 44 -> if (large) 22.sp else 18.sp
            else -> if (large) 18.sp else 15.sp
        }
        SecretText(
            state.value.ifEmpty { " " },
            style = PassType.secret.copy(fontSize = size, lineHeight = size * 1.34f, textAlign = TextAlign.Center),
            modifier = Modifier.padding(horizontal = 18.dp),
        )
        // The dice peeks out above as you pull.
        Icon(
            PassIcons.CasinoFilled,
            null,
            tint = if (armed) colors.primary else colors.textLow,
            modifier = Modifier
                .align(Alignment.TopCenter)
                .size(24.dp)
                .graphicsLayer {
                    translationY = -pull.value * 0.7f
                    alpha = (pull.value / threshold).coerceIn(0f, 1f)
                    rotationZ = pull.value * 2.2f
                },
        )
    }
}

/** How strong the value is: a bar, its bits, and how long it would hold. */
@Composable
fun GeneratedStrength(state: GenState, modifier: Modifier = Modifier) {
    val colors = palette
    var strength by remember { mutableStateOf<StrengthView?>(null) }
    LaunchedEffect(state.value) {
        delay(90)
        val bytes = state.value.toByteArray()
        strength = runCatching { Pass.strength(bytes) }.getOrNull()
        bytes.fill(0)
    }
    Column(modifier.fillMaxWidth()) {
        StrengthBar(strength?.score ?: 0, Modifier.fillMaxWidth())
        Spacer(Modifier.height(10.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            AnimatedContent(
                strength?.crackSlowHash.orEmpty(),
                modifier = Modifier.weight(1f),
                transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(90)) },
                label = "crack",
            ) { crack ->
                Text(
                    if (crack.isEmpty()) " " else "Time to crack: $crack",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            if (com.wasimaster.wmpass.app.Prefs.of(androidx.compose.ui.platform.LocalContext.current).genShowBits.value) {
                Text(
                    "${state.bits.roundToInt()} bits",
                    color = colors.textLow,
                    style = MaterialTheme.typography.labelMedium.merge(PassType.numeric),
                )
            }
        }
    }
}

/**
 * A whole number you drag: every value passed gives a small tick, and the handle carries the
 * number so a thumb never hides what it is choosing.
 */
@Composable
fun StepDial(label: String, value: Int, range: IntRange, modifier: Modifier = Modifier, onChange: (Int) -> Unit, onSettle: () -> Unit = {}) {
    val colors = palette
    val shell = LocalShell.current
    var dragging by remember { mutableStateOf(false) }
    val lift by androidx.compose.animation.core.animateFloatAsState(if (dragging) 1f else 0f, Springs.snap(), label = "lift")
    if (colors.flat) {
        Column(modifier.fillMaxWidth()) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(label, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Text(value.toString(), color = colors.textMid, style = MaterialTheme.typography.bodyLarge.merge(PassType.numeric))
            }
            androidx.compose.material3.Slider(
                value = value.toFloat(),
                onValueChange = {
                    val next = it.roundToInt()
                    if (next != value) {
                        shell.haptics.tick()
                        onChange(next)
                    }
                },
                valueRange = range.first.toFloat()..range.last.toFloat(),
                onValueChangeFinished = onSettle,
            )
        }
        return
    }
    Column(modifier.fillMaxWidth()) {
        Text(label, color = colors.textMid, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(start = 2.dp, bottom = 8.dp))
        BoxWithConstraints(Modifier.fillMaxWidth().height(44.dp)) {
            val width = constraints.maxWidth.toFloat()
            val thumb = with(LocalDensity.current) { 44.dp.toPx() }
            val travel = width - thumb
            val span = (range.last - range.first).coerceAtLeast(1)
            val fraction = (value - range.first).toFloat() / span
            fun fromX(x: Float): Int = range.first + (((x - thumb / 2) / travel).coerceIn(0f, 1f) * span).roundToInt()
            var current by remember { mutableFloatStateOf(0f) }
            Box(
                Modifier
                    .fillMaxSize()
                    .pointerInput(range) {
                        detectDragGestures(
                            onDragStart = {
                                dragging = true
                                current = it.x
                                val next = fromX(it.x)
                                if (next != value) onChange(next)
                            },
                            onDragEnd = {
                                dragging = false
                                onSettle()
                            },
                            onDragCancel = {
                                dragging = false
                                onSettle()
                            },
                        ) { change, delta ->
                            change.consume()
                            current += delta.x
                            val next = fromX(current)
                            onChange(next)
                        }
                    },
            ) {
                Canvas(Modifier.fillMaxSize()) {
                    val h = 6.dp.toPx()
                    val y = (size.height - h) / 2
                    drawRoundRect(colors.surface3, Offset(thumb / 2, y), Size(travel, h), CornerRadius(h / 2))
                    drawRoundRect(
                        Brush.horizontalGradient(listOf(colors.brandTo, colors.brandFrom), startX = thumb / 2, endX = thumb / 2 + travel),
                        Offset(thumb / 2, y),
                        Size(travel * fraction, h),
                        CornerRadius(h / 2),
                    )
                }
                Box(
                    Modifier
                        .graphicsLayer {
                            translationX = travel * fraction
                            val scale = 1f + lift * 0.14f
                            scaleX = scale
                            scaleY = scale
                        }
                        .size(44.dp)
                        .clip(CircleShape)
                        .background(colors.surface3)
                        .rim(CircleShape, colors),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        value.toString(),
                        color = colors.text,
                        style = MaterialTheme.typography.labelLarge.merge(PassType.numeric).copy(fontWeight = FontWeight.Bold),
                    )
                }
            }
        }
    }
    LaunchedEffect(value) {
        if (dragging) shell.haptics.tick()
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun GeneratorControls(state: GenState, modesModifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    fun reroll() { scope.launch { state.roll() } }
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        LazyRow(
            modesModifier.then(if (palette.flat) Modifier.bleed(16.dp) else Modifier.bleed(16.dp).fadeEdges(16.dp)),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            items(Modes, key = { it.first }) { (id, label) ->
                Pill(label, state.mode == id) {
                    state.mode = id
                    reroll()
                }
            }
        }
        when (state.mode) {
            "password" -> {
                StepDial("Length", state.length, 8..BUFFER, onChange = { state.length = it }, onSettle = { scope.launch { state.settle() } })
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill("a-z", state.lower) { state.lower = !state.lower; reroll() }
                    Pill("A-Z", state.upper) { state.upper = !state.upper; reroll() }
                    Pill("0-9", state.digits) { state.digits = !state.digits; reroll() }
                    Pill("!@#", state.symbols) { state.symbols = !state.symbols; reroll() }
                    Pill("Avoid look-alikes", state.noAmbiguous) { state.noAmbiguous = !state.noAmbiguous; reroll() }
                }
            }
            "passphrase" -> {
                StepDial("Words", state.words, 3..12, onChange = { state.words = it }, onSettle = { reroll() })
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("-" to "Dash", " " to "Space", "." to "Dot", "_" to "Underscore").forEach { (sep, name) ->
                        Pill(name, state.separator == sep) { state.separator = sep; reroll() }
                    }
                    Pill("Capitals", state.capitalize) { state.capitalize = !state.capitalize; reroll() }
                    Pill("Number", state.includeDigit) { state.includeDigit = !state.includeDigit; reroll() }
                }
            }
            "pronounceable" -> StepDial("Syllables", state.syllables, 3..14, onChange = { state.syllables = it }, onSettle = { reroll() })
            "pin" -> StepDial("Digits", state.pin, 4..12, onChange = { state.pin = it }, onSettle = { reroll() })
            else -> {
                val field = rememberTextFieldState(state.pattern)
                LaunchedEffect(field.text) {
                    state.pattern = field.text.toString()
                    delay(200)
                    runCatching { state.roll() }
                }
                SheetField(field, "Cvcv-####")
                Text(
                    "C is a consonant, v a vowel, # a digit, ! a symbol. Anything else stays as typed.",
                    color = palette.textMid,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(start = 6.dp),
                )
            }
        }
    }
}

/** Rolls with a spin of the dice. */
@Composable
fun RollButton(onRoll: () -> Unit, modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val spin = remember { Animatable(0f) }
    val source = remember { MutableInteractionSource() }
    if (colors.flat) {
        androidx.compose.material3.FilledTonalIconButton(
            onClick = {
                shell.haptics.click()
                onRoll()
            },
            modifier = modifier.size(48.dp),
        ) { Icon(PassIcons.Refresh, "Generate again") }
        return
    }
    Box(
        modifier
            .pressScale(source, 0.9f)
            .size(56.dp)
            .clip(CircleShape)
            .background(colors.surface2)
            .rim(CircleShape, colors)
            .clickable(source, androidx.compose.material3.ripple(), onClickLabel = "Roll again") {
                shell.haptics.click()
                onRoll()
                scope.launch {
                    spin.animateTo(spin.value + 360f, Springs.glide())
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(PassIcons.Casino, "Roll again", tint = colors.text, modifier = Modifier.size(25.dp).graphicsLayer { rotationZ = spin.value })
    }
}

@Composable
fun GeneratorTab() {
    val colors = palette
    val shell = LocalShell.current
    val prefs = LocalShell.current.prefs
    val state = remember { GenState.fromPrefs(prefs) }
    val scope = rememberCoroutineScope()
    var presets by remember { mutableStateOf<List<PresetView>>(emptyList()) }
    LaunchedEffect(Unit) {
        state.roll()
        presets = runCatching { Pass.presets() }.getOrDefault(emptyList())
    }
    Box(Modifier.fillMaxSize()) {
        Aurora(Modifier.fillMaxWidth().height(460.dp), strength = if (colors.dark) 0.55f else 0.7f)
        Box(
            Modifier
                .fillMaxWidth()
                .height(460.dp)
                .background(Brush.verticalGradient(0.4f to Color.Transparent, 1f to colors.ground)),
        )
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .then(if (colors.flat) Modifier else Modifier.statusBarsPadding())
                .padding(horizontal = 16.dp),
        ) {
            if (!colors.flat) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Generate",
                        color = colors.text,
                        style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp),
                        modifier = Modifier.weight(1f).padding(start = 6.dp, top = 20.dp, bottom = 6.dp),
                    )
                    HelpAction("screen.generator", "the generator", Modifier.padding(top = 14.dp), tint = colors.text)
                }
            }
            GeneratedValue(state, Modifier.padding(vertical = if (colors.flat) 8.dp else 18.dp).helpAnchor("generator.value"))
            GeneratedStrength(state, Modifier.padding(horizontal = if (colors.flat) 0.dp else 6.dp))
            Spacer(Modifier.height(20.dp))
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PrimaryButton("Copy", Modifier.weight(1f), icon = PassIcons.ContentCopy) {
                    shell.copyGenerated(state.value)
                }
                if (!colors.flat) RollButton({ scope.launch { state.roll() } })
                val source = remember { MutableInteractionSource() }
                if (colors.flat) {
                    com.wasimaster.wmpass.ui.parts.QuietButton("Save as login", Modifier.weight(1f).helpAnchor("generator.save"), PassIcons.Add, height = 48.dp) {
                        shell.pendingPassword = state.value
                        shell.nav.navigate("new/login")
                    }
                } else Box(
                    Modifier
                        .helpAnchor("generator.save")
                        .pressScale(source, 0.9f)
                        .size(56.dp)
                        .clip(CircleShape)
                        .background(colors.surface2)
                        .rim(CircleShape, colors)
                        .clickable(source, androidx.compose.material3.ripple(), onClickLabel = "Save as a new login") {
                            shell.haptics.click()
                            shell.pendingPassword = state.value
                            shell.nav.navigate("new/login")
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(PassIcons.Add, "Save as a new login", tint = colors.text, modifier = Modifier.size(25.dp))
                }
            }
            Eyebrow(if (colors.flat) "Options" else "Shape")
            Column(
                if (colors.flat) {
                    Modifier.fillMaxWidth().helpAnchor("generator.controls")
                } else {
                    Modifier
                        .helpAnchor("generator.controls")
                        .fillMaxWidth()
                        .clip(Shapes.card)
                        .background(colors.surface1)
                        .rim(Shapes.card, colors)
                        .padding(16.dp)
                },
            ) {
                GeneratorControls(state, Modifier.helpAnchor("generator.modes"))
            }
            if (presets.isNotEmpty()) {
                Eyebrow("Presets")
                LazyRow(
                    if (colors.flat) Modifier.bleed(16.dp) else Modifier.bleed(16.dp).fadeEdges(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(horizontal = 16.dp),
                ) {
                    items(presets, key = { it.name }) { preset ->
                        Pill(preset.name, false) {
                            state.apply(preset)
                            scope.launch { state.roll() }
                        }
                    }
                }
            }
            Spacer(Modifier.height(if (colors.flat) 24.dp else 170.dp))
        }
    }
}

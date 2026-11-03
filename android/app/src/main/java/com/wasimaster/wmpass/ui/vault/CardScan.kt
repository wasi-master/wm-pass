package com.wasimaster.wmpass.ui.vault

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Rect
import android.os.SystemClock
import android.util.Size as CameraSize
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.core.CardReader
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.theme.palette
import java.util.concurrent.Executors

/** The card's shape in the viewfinder: the reader's input, 600 by 375. A real card is 1.586. */
private const val CardAspect = 1.6f

/** Stripe CardScan's digit detector, widened to float32. See `data/cardscan`. */
private const val CardModel = "darknite.tflite"

/** With Google's reader, how long to keep looking for the date and name once the number is in. */
private const val MoreLookMs = 1500L

/**
 * Point the camera at a payment card and get its number, and with Google's reader also its
 * expiry date and name. Frames are read on the phone and dropped; none is kept. A dialog,
 * like [ScanDialog], so the editor underneath keeps what it holds.
 */
@Composable
fun CardScanDialog(onCard: (CardReader.Read) -> Unit, onClose: () -> Unit) {
    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val prefs = LocalShell.current.prefs
        var reader by remember { mutableStateOf(prefs.cardReader.value) }
        // Only builds with Google's SDKs have a choice to make, and they ask before the first scan.
        if (BuildConfig.GOOGLE_SDKS && reader !in setOf(CardReaders.Google, CardReaders.Own)) {
            ReaderChoice(
                onPick = {
                    prefs.cardReader.set(it)
                    reader = it
                },
                onClose = onClose,
            )
        } else {
            CardScanScreen(google = BuildConfig.GOOGLE_SDKS && reader == CardReaders.Google, onCard, onClose)
        }
    }
}

/** The values of `Prefs.cardReader`. Empty means not chosen yet. */
object CardReaders {
    const val Google = "google"
    const val Own = "own"
}

/** What each reader does well and less well, for the first-scan choice and the setting. */
internal object CardReaderTradeOffs {
    val google = listOf(
        true to "Reads the number, the expiry date and the name.",
        true to "Runs on this phone. The picture and what it reads never leave it.",
        false to "Google's library sends Google statistics about how it runs, with details of the phone and app.",
        false to "Play services fetches it the first time, so the first scan may use WM Pass's own reader.",
    )
    val own = listOf(
        true to "Sends nothing to anyone.",
        true to "Built into WM Pass, from Stripe's open card scanner.",
        false to "Reads the card number only. You type the expiry date and the name.",
    )
}

@Composable
private fun ReaderChoice(onPick: (String) -> Unit, onClose: () -> Unit) {
    val colors = palette
    // Nothing chosen for the owner: Continue waits for a pick.
    var picked by remember { mutableStateOf<String?>(null) }
    Box(Modifier.fillMaxSize().background(colors.ground)) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).statusBarsPadding().navigationBarsPadding()
                .padding(horizontal = 20.dp, vertical = 16.dp),
        ) {
            Spacer(Modifier.height(56.dp))
            Text("Choose a card reader", color = colors.text, style = MaterialTheme.typography.headlineSmall)
            Text(
                "Both read the card on this phone. You can change this later in Settings, Editor.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = 6.dp, bottom = 16.dp),
            )
            ReaderOption("Google's reader (ML Kit)", CardReaderTradeOffs.google, picked == CardReaders.Google) { picked = CardReaders.Google }
            Spacer(Modifier.height(12.dp))
            ReaderOption("WM Pass's own reader", CardReaderTradeOffs.own, picked == CardReaders.Own) { picked = CardReaders.Own }
            PrimaryButton("Continue", Modifier.fillMaxWidth().padding(top = 24.dp), enabled = picked != null) { picked?.let(onPick) }
        }
        GlassCircle(PassIcons.Close, "Close", Modifier.statusBarsPadding().padding(14.dp), onClick = onClose)
    }
}

@Composable
private fun ReaderOption(title: String, points: List<Pair<Boolean, String>>, selected: Boolean, onSelect: () -> Unit) {
    val colors = palette
    GroupCard(Modifier.selectable(selected, role = Role.RadioButton, onClick = onSelect)) {
        Row(Modifier.padding(start = 6.dp, end = 16.dp, top = 6.dp, bottom = 14.dp)) {
            RadioButton(selected, onClick = null, modifier = Modifier.padding(12.dp), colors = RadioButtonDefaults.colors(selectedColor = colors.primary))
            Column(Modifier.weight(1f).padding(top = 10.dp)) {
                Text(title, color = colors.text, style = MaterialTheme.typography.titleMedium)
                points.forEach { (good, text) ->
                    TradeOff(if (good) PassIcons.Check else PassIcons.Info, if (good) colors.primary else colors.warn, text)
                }
            }
        }
    }
}

@Composable
internal fun TradeOff(icon: ImageVector, tint: Color, text: String) {
    val colors = palette
    Row(Modifier.padding(top = 8.dp)) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(10.dp))
        Text(text, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun CardScanScreen(google: Boolean, onCard: (CardReader.Read) -> Unit, onClose: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    var problem by remember { mutableStateOf<String?>(null) }
    // Whether the reader running reads the date and name too, and whether the number is in.
    var readsAll by remember { mutableStateOf(google) }
    var numberIn by remember { mutableStateOf(false) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed = it }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        if (allowed) {
            CardFeed(
                google = google,
                onEngine = { all, note ->
                    readsAll = all
                    if (note != null) problem = note
                },
                onNumber = { numberIn = true },
                onRead = { read ->
                    shell.haptics.confirm()
                    onCard(read)
                },
                onFailed = { problem = "The card reader would not start. Type the number instead." },
            )
            Viewfinder(CardAspect)
        } else {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "The camera is only for reading the card, on this phone. You can also type it by hand.",
                    color = colors.text,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                )
                PrimaryButton("Allow the camera", Modifier.padding(top = 20.dp)) { ask.launch(Manifest.permission.CAMERA) }
            }
        }
        GlassCircle(PassIcons.Close, "Close", Modifier.statusBarsPadding().padding(14.dp), onClick = onClose)
        if (allowed) {
            Column(
                Modifier.align(Alignment.BottomCenter).fillMaxWidth().navigationBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 28.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    if (numberIn) "Got the number. Hold still for the date and name" else "Fit the card in the frame",
                    color = Color.White,
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    problem ?: if (readsAll) "Reads the number, expiry date and name." else "Reads the card number. The expiry date and name are yours to type.",
                    color = if (problem != null) colors.warn else Color.White.copy(alpha = 0.75f),
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
                )
            }
        }
    }
}

/** One way of reading a card from camera frames. Every call comes from the camera's thread. */
internal interface CardEngine {
    /** Gets ready. False when it cannot run on this phone right now. */
    fun open(context: Context): Boolean

    /** One RGBA frame, looking only inside [crop] of the upright frame. Null until three frames agree on a number. */
    fun read(image: ImageProxy, crop: FloatArray): CardReader.Read?

    /** Lets go of everything, wiping what it can of the frames. */
    fun close()
}

/** WM Pass's own reader: Stripe CardScan's digit detector, run by the Rust core. */
private class OwnCardEngine : CardEngine {
    private var opened = false
    private var pixels = ByteArray(0)

    override fun open(context: Context): Boolean {
        val model = runCatching { context.assets.open(CardModel).use { it.readBytes() } }.getOrNull() ?: return false
        opened = CardReader.open(model)
        return opened
    }

    override fun read(image: ImageProxy, crop: FloatArray): CardReader.Read? {
        val plane = image.planes[0]
        val buffer = plane.buffer
        buffer.rewind()
        val size = buffer.remaining()
        if (pixels.size < size) {
            pixels.fill(0)
            pixels = ByteArray(size)
        }
        buffer.get(pixels, 0, size)
        return CardReader.frame(pixels, image.width, image.height, plane.rowStride, image.imageInfo.rotationDegrees, crop)
    }

    override fun close() {
        if (opened) CardReader.close()
        pixels.fill(0)
    }
}

@Composable
private fun CardFeed(
    google: Boolean,
    onEngine: (readsAll: Boolean, note: String?) -> Unit,
    onNumber: () -> Unit,
    onRead: (CardReader.Read) -> Unit,
    onFailed: () -> Unit,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val worker = remember { Executors.newSingleThreadExecutor() }
    // Shared with the camera's thread.
    val held = remember {
        object {
            var provider: ProcessCameraProvider? = null
            @Volatile var closed = false
            @Volatile var done = false
            @Volatile var viewWidth = 0
            @Volatile var viewHeight = 0
            var engine: CardEngine? = null
            var readsAll = false
            var numberAt = 0L
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            held.closed = true
            held.provider?.unbindAll()
            worker.execute { held.engine?.close() }
            worker.shutdown()
        }
    }
    val main = ContextCompat.getMainExecutor(context)
    AndroidView(
        modifier = Modifier.fillMaxSize().onSizeChanged {
            held.viewWidth = it.width
            held.viewHeight = it.height
        },
        factory = { ctx ->
            val view = PreviewView(ctx)
            val future = ProcessCameraProvider.getInstance(ctx)
            future.addListener(
                {
                    if (held.closed) return@addListener
                    val provider = future.get()
                    held.provider = provider
                    // Preview and frames at the same 16:9, so the window drawn over the preview
                    // marks the same part of each frame. Full HD keeps a card's digits sharp.
                    val wide = AspectRatioStrategy.RATIO_16_9_FALLBACK_AUTO_STRATEGY
                    val preview = Preview.Builder()
                        .setResolutionSelector(ResolutionSelector.Builder().setAspectRatioStrategy(wide).build())
                        .build()
                        .also { it.surfaceProvider = view.surfaceProvider }
                    val selector = ResolutionSelector.Builder()
                        .setAspectRatioStrategy(wide)
                        .setResolutionStrategy(ResolutionStrategy(CameraSize(1920, 1080), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER))
                        .build()
                    val analysis = ImageAnalysis.Builder()
                        .setResolutionSelector(selector)
                        .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .build()
                    analysis.setAnalyzer(worker) { image ->
                        image.use {
                            if (held.closed || held.done) return@use
                            var engine = held.engine
                            if (engine == null) {
                                engine = start(ctx, google, { held.readsAll = it }, onEngine, main)
                                if (engine == null) {
                                    held.done = true
                                    main.execute { onFailed() }
                                    return@use
                                }
                                held.engine = engine
                            }
                            val rotation = it.imageInfo.rotationDegrees
                            val crop = cardCrop(held.viewWidth, held.viewHeight, it.width, it.height, rotation)
                            val read = engine.read(it, crop) ?: return@use
                            if (held.closed) return@use
                            val now = SystemClock.elapsedRealtime()
                            if (held.numberAt == 0L) {
                                held.numberAt = now
                                if (held.readsAll) main.execute { onNumber() }
                            }
                            // Google's reader also reads the date and name: give them a moment to agree too.
                            if (!held.readsAll || (read.expiry != null && read.name != null) || now - held.numberAt >= MoreLookMs) {
                                held.done = true
                                main.execute { onRead(read) }
                            }
                        }
                    }
                    provider.unbindAll()
                    provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                },
                main,
            )
            view
        },
    )
}

/**
 * Starts the reader asked for, falling back to WM Pass's own when Google's cannot run yet,
 * and says which one runs. Null when neither starts.
 */
private fun start(
    context: Context,
    google: Boolean,
    readsAll: (Boolean) -> Unit,
    onEngine: (Boolean, String?) -> Unit,
    main: java.util.concurrent.Executor,
): CardEngine? {
    if (google) {
        val engine = MlKitCards.engine()
        if (engine != null && engine.open(context)) {
            readsAll(true)
            main.execute { onEngine(true, null) }
            return engine
        }
    }
    val own = OwnCardEngine().takeIf { it.open(context) } ?: return null
    readsAll(false)
    val note = if (google) "Google's reader is not ready yet, so WM Pass's own reader is reading this card." else null
    main.execute { onEngine(false, note) }
    return own
}

/**
 * Where the viewfinder's window falls in a frame, as left, top, right and bottom fractions of
 * the upright frame. The preview fills the view and is cut to it at the center, so the frame
 * is scaled by whichever side fills first.
 */
private fun cardCrop(viewWidth: Int, viewHeight: Int, width: Int, height: Int, rotation: Int): FloatArray {
    val (frameWidth, frameHeight) = if (rotation % 180 == 90) height to width else width to height
    if (viewWidth <= 0 || viewHeight <= 0) return floatArrayOf(0f, 0f, 1f, 1f)
    val scale = maxOf(viewWidth.toFloat() / frameWidth, viewHeight.toFloat() / frameHeight)
    val window = finderSize(viewWidth.toFloat(), viewHeight.toFloat(), CardAspect)
    val halfWide = (window.width / scale / frameWidth / 2).coerceAtMost(0.5f)
    val halfTall = (window.height / scale / frameHeight / 2).coerceAtMost(0.5f)
    return floatArrayOf(0.5f - halfWide, 0.5f - halfTall, 0.5f + halfWide, 0.5f + halfTall)
}

/**
 * [crop], fractions of the upright frame, as pixels of the [width] by [height] frame as the
 * camera gave it, before turning it [rotation] degrees clockwise. Null when nothing is left.
 */
internal fun sensorRect(crop: FloatArray, width: Int, height: Int, rotation: Int): Rect? {
    val (left, top, right, bottom) = crop.toList()
    val (x0, y0, x1, y1) = when (rotation) {
        90 -> listOf(top, 1 - right, bottom, 1 - left)
        180 -> listOf(1 - right, 1 - bottom, 1 - left, 1 - top)
        270 -> listOf(1 - bottom, left, 1 - top, right)
        else -> listOf(left, top, right, bottom)
    }
    val rect = Rect((x0 * width).toInt(), (y0 * height).toInt(), (x1 * width).toInt(), (y1 * height).toInt())
    return rect.takeIf { it.intersect(0, 0, width, height) && !it.isEmpty }
}

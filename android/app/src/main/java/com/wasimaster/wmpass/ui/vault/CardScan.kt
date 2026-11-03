package com.wasimaster.wmpass.ui.vault

import android.Manifest
import android.content.pm.PackageManager
import android.util.Size as CameraSize
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import com.wasimaster.wmpass.core.CardReader
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.theme.palette
import java.util.concurrent.Executors

/** The card's shape in the viewfinder: the reader's input, 600 by 375. A real card is 1.586. */
private const val CardAspect = 1.6f

/** Stripe CardScan's digit detector, widened to float32. See `data/cardscan`. */
private const val CardModel = "darknite.tflite"

/**
 * Point the camera at a payment card and get its number. Frames are read on the phone by the
 * Rust core and dropped; none is kept and nothing is sent. A dialog, like [ScanDialog], so the
 * editor underneath keeps what it holds.
 */
@Composable
fun CardScanDialog(onCard: (CardReader.Read) -> Unit, onClose: () -> Unit) {
    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        CardScanScreen(onCard, onClose)
    }
}

@Composable
private fun CardScanScreen(onCard: (CardReader.Read) -> Unit, onClose: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    var problem by remember { mutableStateOf<String?>(null) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed = it }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        if (allowed) {
            CardFeed(
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
                    "The camera is only for reading the card number, on this phone. You can also type it by hand.",
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
                    problem ?: "Fit the card in the frame",
                    color = if (problem != null) colors.danger else Color.White,
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    "Reads the card number. The expiry date and name are yours to type.",
                    color = Color.White.copy(alpha = 0.75f),
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
                )
            }
        }
    }
}

@Composable
private fun CardFeed(onRead: (CardReader.Read) -> Unit, onFailed: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val worker = remember { Executors.newSingleThreadExecutor() }
    // Shared with the camera's thread. The frame buffer is wiped once the reader closes.
    val held = remember {
        object {
            var provider: ProcessCameraProvider? = null
            @Volatile var closed = false
            @Volatile var done = false
            @Volatile var viewWidth = 0
            @Volatile var viewHeight = 0
            var opened = false
            var pixels = ByteArray(0)
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            held.closed = true
            held.provider?.unbindAll()
            worker.execute {
                if (held.opened) CardReader.close()
                held.pixels.fill(0)
            }
            worker.shutdown()
        }
    }
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
                            if (!held.opened) {
                                val model = runCatching { ctx.assets.open(CardModel).use { stream -> stream.readBytes() } }.getOrNull()
                                held.opened = model != null && CardReader.open(model)
                                if (!held.opened) {
                                    held.done = true
                                    ContextCompat.getMainExecutor(context).execute { onFailed() }
                                    return@use
                                }
                            }
                            val plane = it.planes[0]
                            val buffer = plane.buffer
                            buffer.rewind()
                            val size = buffer.remaining()
                            if (held.pixels.size < size) {
                                held.pixels.fill(0)
                                held.pixels = ByteArray(size)
                            }
                            buffer.get(held.pixels, 0, size)
                            val rotation = it.imageInfo.rotationDegrees
                            val crop = cardCrop(held.viewWidth, held.viewHeight, it.width, it.height, rotation)
                            val read = CardReader.frame(held.pixels, it.width, it.height, plane.rowStride, rotation, crop)
                            if (read != null && !held.closed) {
                                held.done = true
                                ContextCompat.getMainExecutor(context).execute { onRead(read) }
                            }
                        }
                    }
                    provider.unbindAll()
                    provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                },
                ContextCompat.getMainExecutor(ctx),
            )
            view
        },
    )
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

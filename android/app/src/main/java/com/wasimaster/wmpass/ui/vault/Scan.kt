package com.wasimaster.wmpass.ui.vault

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.GlobalHistogramBinarizer
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.multi.qrcode.QRCodeMultiReader
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import java.util.concurrent.Executors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Point the camera at a QR code, or pick a picture of one. The picture goes nowhere and nothing is
 * stored. A dialog rather than a page, so the screen or sheet that asked stays as it was and gets
 * the code: a page would have taken its place and dropped what it held.
 */
@Composable
fun ScanDialog(onCode: (String) -> Unit, onClose: () -> Unit) {
    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        ScanScreen(onCode, onClose)
    }
}

@Composable
private fun ScanScreen(onCode: (String) -> Unit, onClose: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var allowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    var problem by remember { mutableStateOf<String?>(null) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed = it }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    val done: (String) -> Unit = { text ->
        shell.haptics.confirm()
        onCode(text)
    }
    // The system's photo picker: no storage permission, and only the picture chosen is shared.
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            scope.launch {
                val found = qrCodesIn(context, uri).firstOrNull()
                if (found != null) done(found) else problem = "No QR code was found in that picture."
            }
        }
    }
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        if (allowed) {
            CameraFeed(done)
            Viewfinder()
        } else {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "The camera is only for reading the code. You can also pick a picture of it, or type the setup key by hand.",
                    color = colors.text,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                )
                PrimaryButton("Allow the camera", Modifier.padding(top = 20.dp)) { ask.launch(Manifest.permission.CAMERA) }
            }
        }
        GlassCircle(PassIcons.Close, "Close", Modifier.statusBarsPadding().padding(14.dp), onClick = onClose)
        Column(
            Modifier.align(Alignment.BottomCenter).fillMaxWidth().navigationBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                problem ?: if (allowed) "Point at the QR code" else "",
                color = if (problem != null) colors.danger else Color.White,
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(bottom = 18.dp),
            )
            QuietButton("Pick a picture of it", icon = PassIcons.Image) {
                problem = null
                pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
            }
        }
    }
}

@Composable
private fun CameraFeed(onCode: (String) -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val worker = remember { Executors.newSingleThreadExecutor() }
    var found by remember { mutableStateOf(false) }
    // Bound to the page underneath, which outlives this dialog, so it lets go of the camera itself.
    val held = remember { object { var provider: ProcessCameraProvider? = null; var closed = false } }
    DisposableEffect(Unit) {
        onDispose {
            held.closed = true
            held.provider?.unbindAll()
            worker.shutdown()
        }
    }
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            val view = PreviewView(ctx)
            val future = ProcessCameraProvider.getInstance(ctx)
            future.addListener(
                {
                    if (held.closed) return@addListener
                    val provider = future.get()
                    held.provider = provider
                    val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                    val reader = MultiFormatReader().apply {
                        setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE)))
                    }
                    val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                    analysis.setAnalyzer(worker) { image ->
                        image.use {
                            val plane = it.planes[0]
                            val buffer = plane.buffer
                            val data = ByteArray(buffer.remaining())
                            buffer.get(data)
                            val source = PlanarYUVLuminanceSource(data, plane.rowStride, it.height, 0, 0, it.width, it.height, false)
                            val text = runCatching { reader.decodeWithState(BinaryBitmap(HybridBinarizer(source))).text }.getOrNull()
                            reader.reset()
                            if (text != null && !found) {
                                found = true
                                ContextCompat.getMainExecutor(context).execute { onCode(text) }
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

/** A window cut out of a dimmed picture, with a line sweeping it. Square, or [aspect] wide to one tall. */
@Composable
internal fun Viewfinder(aspect: Float = 1f) {
    val colors = palette
    val sweep by rememberInfiniteTransition(label = "sweep").animateFloat(
        0f,
        1f,
        infiniteRepeatable(tween(1800), RepeatMode.Reverse),
        label = "line",
    )
    Canvas(Modifier.fillMaxSize().graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }) {
        val window = finderSize(size.width, size.height, aspect)
        val at = Offset((size.width - window.width) / 2, (size.height - window.height) / 2)
        val radius = CornerRadius((if (aspect == 1f) 30.dp else 18.dp).toPx())
        drawRect(Color.Black.copy(alpha = 0.58f))
        drawRoundRect(Color.Black, at, window, radius, blendMode = BlendMode.Clear)
        drawRoundRect(colors.primary, at, window, radius, style = Stroke(3.dp.toPx()))
        val y = at.y + 18.dp.toPx() + (window.height - 36.dp.toPx()) * sweep
        drawLine(colors.primary, Offset(at.x + 18.dp.toPx(), y), Offset(at.x + window.width - 18.dp.toPx(), y), 2.dp.toPx(), StrokeCap.Round)
    }
}

/** The window [Viewfinder] cuts, centered in a [width] by [height] view. */
internal fun finderSize(width: Float, height: Float, aspect: Float): Size {
    if (aspect == 1f) return (minOf(width, height) * 0.68f).let { Size(it, it) }
    val wide = minOf(width * 0.9f, height * 0.6f * aspect)
    return Size(wide, wide / aspect)
}

/** Every QR code in a picture, such as a screenshot of another app's export. Empty when there is none. */
suspend fun qrCodesIn(context: Context, uri: Uri): List<String> = withContext(Dispatchers.Default) {
    val bitmap = runCatching { loadForReading(context, uri) }.getOrNull() ?: return@withContext emptyList()
    val pixels = IntArray(bitmap.width * bitmap.height)
    bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
    val source = RGBLuminanceSource(bitmap.width, bitmap.height, pixels)
    bitmap.recycle()
    val hints = mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE), DecodeHintType.TRY_HARDER to true)
    // A light code on a dark screen reads only once inverted; a photo can need the other binarizer.
    for (candidate in listOf(source, source.invert())) {
        for (binary in listOf(BinaryBitmap(HybridBinarizer(candidate)), BinaryBitmap(GlobalHistogramBinarizer(candidate)))) {
            val found = runCatching { QRCodeMultiReader().decodeMultiple(binary, hints) }.getOrNull()
            if (!found.isNullOrEmpty()) return@withContext found.map { it.text }.distinct()
        }
    }
    emptyList()
}

/** At most [ReadSide] pixels on its longer side: plenty for a code, without a camera photo's full weight. */
private fun loadForReading(context: Context, uri: Uri): Bitmap? {
    val resolver = context.contentResolver
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    // Measures only, and so always comes back null itself.
    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > ReadSide) sample *= 2
    val options = BitmapFactory.Options().apply {
        inSampleSize = sample
        inPreferredConfig = Bitmap.Config.ARGB_8888
    }
    return resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) }
}

private const val ReadSide = 2048

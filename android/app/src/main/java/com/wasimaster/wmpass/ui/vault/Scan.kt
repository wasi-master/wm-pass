package com.wasimaster.wmpass.ui.vault

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
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
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.theme.palette
import java.util.concurrent.Executors

/** Point the camera at a QR code. The picture goes nowhere and nothing is stored. */
@Composable
fun ScanScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed = it }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    val done: (String) -> Unit = { text ->
        shell.haptics.confirm()
        shell.scanTarget?.invoke(text)
        shell.scanTarget = null
        shell.nav.popBackStack()
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
                    "The camera is only for reading the code. You can also type the setup key by hand.",
                    color = colors.text,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                )
                PrimaryButton("Allow the camera", Modifier.padding(top = 20.dp)) { ask.launch(Manifest.permission.CAMERA) }
            }
        }
        GlassCircle(PassIcons.Close, "Close", Modifier.statusBarsPadding().padding(14.dp)) { shell.nav.popBackStack() }
        Text(
            "Point at the QR code",
            color = Color.White,
            style = MaterialTheme.typography.titleMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(bottom = 96.dp),
        )
    }
}

@Composable
private fun CameraFeed(onCode: (String) -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val worker = remember { Executors.newSingleThreadExecutor() }
    var found by remember { mutableStateOf(false) }
    DisposableEffect(Unit) { onDispose { worker.shutdown() } }
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            val view = PreviewView(ctx)
            val future = ProcessCameraProvider.getInstance(ctx)
            future.addListener(
                {
                    val provider = future.get()
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

/** A window cut out of a dimmed picture, with a line sweeping it. */
@Composable
private fun Viewfinder() {
    val colors = palette
    val sweep by rememberInfiniteTransition(label = "sweep").animateFloat(
        0f,
        1f,
        infiniteRepeatable(tween(1800), RepeatMode.Reverse),
        label = "line",
    )
    Canvas(Modifier.fillMaxSize().graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }) {
        val side = size.minDimension * 0.68f
        val at = Offset((size.width - side) / 2, (size.height - side) / 2)
        val radius = CornerRadius(30.dp.toPx())
        drawRect(Color.Black.copy(alpha = 0.58f))
        drawRoundRect(Color.Black, at, Size(side, side), radius, blendMode = BlendMode.Clear)
        drawRoundRect(colors.primary, at, Size(side, side), radius, style = Stroke(3.dp.toPx()))
        val y = at.y + 18.dp.toPx() + (side - 36.dp.toPx()) * sweep
        drawLine(colors.primary, Offset(at.x + 18.dp.toPx(), y), Offset(at.x + side - 18.dp.toPx(), y), 2.dp.toPx(), StrokeCap.Round)
    }
}

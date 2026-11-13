package com.wasimaster.wmpass.wear

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.items
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material3.AppScaffold
import androidx.wear.compose.material3.Button
import androidx.wear.compose.material3.ListHeader
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.ScreenScaffold
import androidx.wear.compose.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val PASSWORD_SHOWN_MS = 20_000L

class MainActivity : ComponentActivity() {
    /** On screen: the phone is asked only then, and a password shown goes when the screen does. */
    private var onScreen by mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        setContent {
            MaterialTheme {
                AppScaffold { Screen(onScreen) }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        onScreen = true
    }

    override fun onStop() {
        onScreen = false
        super.onStop()
    }
}

@Composable
private fun Screen(visible: Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val list = rememberScalingLazyListState()
    var answer by remember { mutableStateOf<PhoneAnswer?>(null) }
    var round by remember { mutableIntStateOf(0) }
    var clock by remember { mutableIntStateOf(0) }
    var shown by remember { mutableStateOf<Pair<String, String>?>(null) }

    // Off screen nothing is asked: the codes would only keep the phone and the watch awake.
    // Back on screen, they are asked for at once rather than shown as they were.
    LaunchedEffect(round, visible) {
        if (!visible) {
            shown = null
            return@LaunchedEffect
        }
        val found = PhoneLink.items(context)
        answer = found
        clock = 0
        // Ask again when the soonest code runs out.
        val soonest = (found as? PhoneAnswer.Open)?.codes?.minOfOrNull { it.secondsLeft }
        if (soonest != null) {
            repeat(soonest.coerceAtLeast(1)) {
                delay(1000)
                clock++
            }
            round++
        }
    }

    LaunchedEffect(shown) {
        if (shown != null) {
            delay(PASSWORD_SHOWN_MS)
            shown = null
        }
    }

    ScreenScaffold(scrollState = list) {
        when (val now = answer) {
            null -> Note("Asking your phone", list)
            // Back to "Asking" on a retry, so the tap is seen to do something while the phone answers.
            PhoneAnswer.Away -> Note("Can't reach your phone", list) { answer = null; round++ }
            PhoneAnswer.Locked -> Note("Unlock WM Pass on your phone", list) { answer = null; round++ }
            PhoneAnswer.Off -> Note("Turn on Show on my watch in WM Pass on your phone", list) { answer = null; round++ }
            is PhoneAnswer.Open -> ScalingLazyColumn(Modifier.fillMaxSize(), state = list) {
                if (now.codes.isEmpty() && now.favourites.isEmpty()) {
                    item { Text("No codes or favorites yet", textAlign = TextAlign.Center) }
                }
                if (now.codes.isNotEmpty()) item { ListHeader { Text("Codes") } }
                items(now.codes, key = { it.id }) { row ->
                    val left = (row.secondsLeft - clock).coerceAtLeast(0)
                    // One stop for a screen reader: the name, the code digit by digit (not as one large
                    // number), and the seconds in words rather than "s".
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(vertical = 6.dp)
                            .clearAndSetSemantics {
                                contentDescription = "${row.title}, code ${row.code.filter { !it.isWhitespace() }.toList().joinToString(" ")}, $left seconds left"
                            },
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Text(row.title, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(row.code, style = MaterialTheme.typography.titleLarge, fontFamily = FontFamily.Monospace, maxLines = 1, softWrap = false)
                        Text("$left s", style = MaterialTheme.typography.labelSmall)
                    }
                }
                if (now.favourites.isNotEmpty()) item { ListHeader { Text("Favorites") } }
                items(now.favourites, key = { "f" + it.id }) { row ->
                    Button(
                        onClick = {
                            if (shown?.first == row.id) {
                                shown = null
                            } else if (row.hasPassword) {
                                scope.launch {
                                    val password = PhoneLink.password(context, row.id)
                                    // None back: the vault locked or the phone went out of reach since
                                    // the list came, so the list is asked for again and says which.
                                    if (password != null) shown = row.id to password else round++
                                }
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column {
                            Text(row.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            val under = shown?.takeIf { it.first == row.id }?.second ?: row.username
                            Text(under, style = MaterialTheme.typography.labelSmall, fontFamily = if (shown?.first == row.id) FontFamily.Monospace else null)
                        }
                    }
                }
            }
        }
    }
}

/**
 * A message, and a way to try again. In a list, so at a large font size it scrolls, by finger or
 * by the crown, rather than running off the round screen; and said as it changes.
 */
@Composable
private fun Note(words: String, list: androidx.wear.compose.foundation.lazy.ScalingLazyListState, onRetry: (() -> Unit)? = null) {
    ScalingLazyColumn(Modifier.fillMaxSize(), state = list) {
        item {
            Text(
                words,
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        if (onRetry != null) {
            item { Button(onClick = onRetry, modifier = Modifier.padding(top = 8.dp)) { Text("Try again") } }
        }
    }
}

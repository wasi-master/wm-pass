package com.wasimaster.wmpass.wear

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.ui.text.style.TextAlign
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
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        setContent {
            MaterialTheme {
                AppScaffold { Screen() }
            }
        }
    }
}

@Composable
private fun Screen() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val list = rememberScalingLazyListState()
    var answer by remember { mutableStateOf<PhoneAnswer?>(null) }
    var round by remember { mutableIntStateOf(0) }
    var clock by remember { mutableIntStateOf(0) }
    var shown by remember { mutableStateOf<Pair<String, String>?>(null) }

    LaunchedEffect(round) {
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
            null -> Note("Asking your phone")
            PhoneAnswer.Away -> Note("Can't reach your phone") { round++ }
            PhoneAnswer.Locked -> Note("Unlock WM Pass on your phone") { round++ }
            PhoneAnswer.Off -> Note("Turn on Show on my watch in WM Pass on your phone") { round++ }
            is PhoneAnswer.Open -> ScalingLazyColumn(Modifier.fillMaxSize(), state = list) {
                if (now.codes.isEmpty() && now.favourites.isEmpty()) {
                    item { Text("No codes or favorites yet", textAlign = TextAlign.Center) }
                }
                if (now.codes.isNotEmpty()) item { ListHeader { Text("Codes") } }
                items(now.codes, key = { it.id }) { row ->
                    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(row.title, style = MaterialTheme.typography.labelMedium, maxLines = 1)
                        Text(row.code, style = MaterialTheme.typography.titleLarge, fontFamily = FontFamily.Monospace)
                        Text("${(row.secondsLeft - clock).coerceAtLeast(0)} s", style = MaterialTheme.typography.labelSmall)
                    }
                }
                if (now.favourites.isNotEmpty()) item { ListHeader { Text("Favorites") } }
                items(now.favourites, key = { "f" + it.id }) { row ->
                    Button(
                        onClick = {
                            if (shown?.first == row.id) {
                                shown = null
                            } else if (row.hasPassword) {
                                scope.launch { PhoneLink.password(context, row.id)?.let { shown = row.id to it } }
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column {
                            Text(row.title, maxLines = 1)
                            val under = shown?.takeIf { it.first == row.id }?.second ?: row.username
                            Text(under, style = MaterialTheme.typography.labelSmall, fontFamily = if (shown?.first == row.id) FontFamily.Monospace else null)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Note(words: String, onRetry: (() -> Unit)? = null) {
    Column(
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(words, textAlign = TextAlign.Center)
        if (onRetry != null) {
            Button(onClick = onRetry, modifier = Modifier.padding(top = 8.dp)) { Text("Try again") }
        }
    }
}

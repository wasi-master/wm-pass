package com.wasimaster.wmpass.ui.vault

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.palette

/**
 * The vault as an ordinary Android app: a search bar on top, a list, a navigation bar and an add
 * button, each where people already expect it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StandardHome() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    BackHandler(shell.selecting) { shell.selection.clear() }
    BackHandler(shell.searching) { shell.searching = false }
    val tabs = prefs.tabs.mapNotNull { id -> AllTabs.firstOrNull { it.id == id } }
    Box(Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = colors.ground,
            topBar = {
                when {
                    shell.selecting -> TopAppBar(
                        title = { Text("${shell.selection.size} selected") },
                        navigationIcon = {
                            IconButton(onClick = { shell.selection.clear() }) { Icon(PassIcons.Close, "Stop selecting") }
                        },
                        actions = { BulkActions() },
                        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                    )
                    shell.tab == "vault" -> VaultSearchBar()
                    else -> TopAppBar(
                        title = { Text(AllTabs.firstOrNull { it.id == shell.tab }?.label.orEmpty()) },
                        actions = {
                            IconButton(onClick = { shell.lockNow() }) { Icon(PassIcons.Lock, "Lock") }
                            IconButton(onClick = { shell.nav.navigate("settings") }) { Icon(PassIcons.Settings, "Settings") }
                        },
                        colors = TopAppBarDefaults.topAppBarColors(containerColor = colors.ground),
                    )
                }
            },
            bottomBar = {
                if (tabs.size > 1 && !shell.selecting) {
                    NavigationBar {
                        tabs.forEach { tab ->
                            val selected = shell.tab == tab.id
                            NavigationBarItem(
                                selected = selected,
                                onClick = {
                                    if (!selected) shell.haptics.tick()
                                    shell.tab = tab.id
                                },
                                icon = { Icon(if (selected) tab.filled else tab.icon, null) },
                                label = { Text(tab.label, maxLines = 1) },
                            )
                        }
                    }
                }
            },
            floatingActionButton = {
                if (!shell.selecting && (shell.tab == "vault" || shell.tab == "codes")) {
                    FloatingActionButton(onClick = {
                        shell.haptics.click()
                        if (shell.tab == "codes") shell.nav.navigate("new/otp") else shell.adding = true
                    }) { Icon(PassIcons.Add, "Add") }
                }
            },
            snackbarHost = { UndoBar() },
        ) { inner ->
            AnimatedContent(
                targetState = shell.tab,
                modifier = Modifier.fillMaxSize().padding(inner),
                transitionSpec = { fadeIn(tween(160, 40)) togetherWith fadeOut(tween(80)) },
                label = "tab",
            ) { tab ->
                when (tab) {
                    "vault" -> VaultTab()
                    "codes" -> CodesTab()
                    "generator" -> GeneratorTab()
                    "health" -> HealthTab()
                    else -> ToolsTab()
                }
            }
        }
        AddLayer()
        SearchLayer()
    }
}

/** The search bar as Material draws it, doubling as the vault page's title. */
@Composable
private fun VaultSearchBar() {
    val shell = LocalShell.current
    val colors = palette
    Surface(
        Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 8.dp)
            .height(56.dp)
            .clip(CircleShape)
            .clickable { shell.searching = true },
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = CircleShape,
    ) {
        Row(Modifier.fillMaxSize().padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(PassIcons.Search, null, tint = colors.textMid)
            Spacer(Modifier.width(12.dp))
            Text("Search vault", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f), maxLines = 1)
            IconButton(onClick = { shell.lockNow() }) { Icon(PassIcons.Lock, "Lock", tint = colors.textMid) }
            IconButton(onClick = { shell.nav.navigate("settings") }) { Icon(PassIcons.Settings, "Settings", tint = colors.textMid) }
        }
    }
}

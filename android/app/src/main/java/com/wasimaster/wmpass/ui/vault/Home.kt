package com.wasimaster.wmpass.ui.vault

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.ui.fx.BackdropSource
import com.wasimaster.wmpass.ui.fx.LocalBackdrop
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.fx.rememberBackdrop
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale

/** A place in the dock. */
data class Tab(val id: String, val label: String, val icon: ImageVector, val filled: ImageVector)

val AllTabs: List<Tab>
    get() = listOf(
        Tab("vault", "Vault", PassIcons.Shield, PassIcons.ShieldFilled),
        Tab("codes", "Codes", PassIcons.Pin, PassIcons.PinFilled),
        Tab("generator", "Generator", PassIcons.Casino, PassIcons.CasinoFilled),
        Tab("health", "Health", PassIcons.EcgHeart, PassIcons.EcgHeartFilled),
        Tab("tools", "Tools", PassIcons.Widgets, PassIcons.WidgetsFilled),
    )

@Composable
fun HomeScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    androidx.compose.runtime.LaunchedEffect(shell.tab) { prefs.lastTab.set(shell.tab) }
    if (colors.flat) {
        StandardHome()
        return
    }
    BackHandler(shell.selecting) { shell.selection.clear() }
    BackHandler(shell.searching) { shell.searching = false }
    // The page as a whole is already a backdrop, for the capsule above it, so glass in here
    // gets none. The dock wants the rows sliding under it, so the tab is a backdrop of its own.
    val backdrop = rememberBackdrop()
    Box(Modifier.fillMaxSize().background(colors.ground)) {
        val order = prefs.tabs
        BackdropSource(backdrop, Modifier.fillMaxSize()) {
        AnimatedContent(
            targetState = shell.tab,
            modifier = Modifier.fillMaxSize(),
            transitionSpec = {
                val forward = order.indexOf(targetState) > order.indexOf(initialState)
                val shift = if (forward) 1 else -1
                ContentTransform(
                    fadeIn(tween(210, 60)) + androidx.compose.animation.slideInHorizontally(tween(300)) { shift * it / 14 },
                    fadeOut(tween(110)) + androidx.compose.animation.slideOutHorizontally(tween(300)) { -shift * it / 22 },
                )
            },
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
        CompositionLocalProvider(LocalBackdrop provides backdrop) {
        // The bar fades the page into the ground behind the dock, so rows never collide with it.
        Box(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .height(176.dp)
                .drawBehind {
                    drawRect(
                        Brush.verticalGradient(
                            0f to Color.Transparent,
                            0.42f to colors.ground.copy(alpha = 0.78f),
                            1f to colors.ground.copy(alpha = 0.97f),
                        ),
                    )
                },
        )
        Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().navigationBarsPadding().padding(horizontal = 14.dp, vertical = 10.dp)) {
            UndoBar(Modifier.padding(bottom = 10.dp))
            Dock()
        }
        AddLayer()
        SearchLayer()
        }
    }
}

@Composable
private fun Dock() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    AnimatedContent(
        targetState = shell.selecting,
        transitionSpec = {
            (slideInVertically(Springs.glide()) { it } + fadeIn(tween(140))) togetherWith
                (slideOutVertically(tween(160)) { it } + fadeOut(tween(100)))
        },
        label = "dock",
    ) { selecting ->
        if (selecting) {
            SelectionBar()
        } else {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (prefs.leftHanded) ActionStack()
                TabsPill(Modifier.weight(1f))
                if (!prefs.leftHanded) ActionStack()
            }
        }
    }
}

/** Search and add, where a thumb already is. */
@Composable
private fun ActionStack() {
    val shell = LocalShell.current
    val colors = palette
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        AnimatedVisibility(
            visible = shell.tab == "vault" || shell.tab == "codes",
            enter = scaleIn(Springs.lively(), 0.4f) + fadeIn(tween(120)),
            exit = scaleOut(tween(120), 0.4f) + fadeOut(tween(100)),
        ) {
            AddButton(Modifier.padding(bottom = 10.dp))
        }
        val source = remember { MutableInteractionSource() }
        Box(
            Modifier
                .pressScale(source, 0.92f)
                .size(64.dp)
                .drawBehind {
                    // A pool of light under the button: it is the way into everything.
                    drawCircle(
                        Brush.radialGradient(
                            listOf(colors.brandFrom.copy(alpha = 0.42f), Color.Transparent),
                            Offset(size.width / 2, size.height * 0.62f),
                            size.width * 0.86f,
                        ),
                        radius = size.width * 0.86f,
                        center = Offset(size.width / 2, size.height * 0.62f),
                    )
                }
                .clip(CircleShape)
                .background(Brush.linearGradient(listOf(colors.brandFrom, colors.brandTo)))
                .rim(CircleShape, colors)
                .clickable(source, androidx.compose.material3.ripple(), role = Role.Button, onClickLabel = "Search") {
                    shell.haptics.click()
                    shell.searching = true
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(PassIcons.Search, "Search", tint = Color.White, modifier = Modifier.size(28.dp))
        }
    }
}

@Composable
private fun TabsPill(modifier: Modifier = Modifier) {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val tabs = prefs.tabs.mapNotNull { id -> AllTabs.firstOrNull { it.id == id } }
    Row(
        modifier
            .height(64.dp)
            .glass(CircleShape)
            .padding(horizontal = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        tabs.forEach { tab -> TabButton(tab, shell.tab == tab.id) }
    }
}

@Composable
private fun TabButton(tab: Tab, selected: Boolean) {
    val shell = LocalShell.current
    val colors = palette
    val source = remember { MutableInteractionSource() }
    val lit by animateFloatAsState(if (selected) 1f else 0f, Springs.glide(), label = "lit")
    val bounce by animateFloatAsState(if (selected) 1f else 0.92f, Springs.lively(), label = "bounce")
    val pad by animateDpAsState(if (selected) 16.dp else 12.dp, Springs.glide(), label = "pad")
    Row(
        Modifier
            .height(52.dp)
            .clip(CircleShape)
            .background(colors.primary.copy(alpha = lit * if (colors.dark) 0.17f else 0.12f))
            .clickable(source, androidx.compose.material3.ripple(), role = Role.Tab, onClickLabel = tab.label) {
                if (shell.tab != tab.id) shell.haptics.tick()
                shell.tab = tab.id
            }
            .padding(horizontal = pad),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            if (selected) tab.filled else tab.icon,
            tab.label,
            tint = if (selected) colors.primary else colors.textMid,
            modifier = Modifier
                .size(23.dp)
                .graphicsLayer {
                    scaleX = bounce
                    scaleY = bounce
                },
        )
        AnimatedVisibility(
            visible = selected,
            enter = expandHorizontally(Springs.glide()) + fadeIn(tween(160, 60)),
            exit = shrinkHorizontally(Springs.glide()) + fadeOut(tween(80)),
        ) {
            Row {
                Spacer(Modifier.width(8.dp))
                Text(tab.label, color = colors.primary, fontSize = 13.5.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            }
        }
    }
}

@Composable
private fun SelectionBar() {
    val shell = LocalShell.current
    val colors = palette
    Row(
        Modifier
            .fillMaxWidth()
            .height(64.dp)
            .glass(CircleShape)
            .padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(PassIcons.Close, "Stop selecting") { shell.selection.clear() }
        AnimatedContent(
            shell.selection.size,
            transitionSpec = {
                val up = targetState > initialState
                (slideInVertically(tween(180)) { if (up) it else -it } + fadeIn(tween(120))) togetherWith
                    (slideOutVertically(tween(180)) { if (up) -it else it } + fadeOut(tween(100)))
            },
            label = "count",
        ) { count ->
            Text("$count", color = colors.text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 8.dp))
        }
        Spacer(Modifier.weight(1f))
        BulkActions()
    }
}

@Composable
fun BarIcon(icon: ImageVector, label: String, tint: Color = palette.text, onClick: () -> Unit) {
    val shell = LocalShell.current
    if (palette.flat) {
        androidx.compose.material3.IconButton(onClick = {
            shell.haptics.tick()
            onClick()
        }) { Icon(icon, label, tint = tint) }
        return
    }
    val source = remember { MutableInteractionSource() }
    Box(
        Modifier
            .pressScale(source, 0.88f)
            .size(46.dp)
            .clip(CircleShape)
            .clickable(source, androidx.compose.material3.ripple(), role = Role.Button, onClickLabel = label) {
                shell.haptics.tick()
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, tint = tint, modifier = Modifier.size(22.dp))
    }
}


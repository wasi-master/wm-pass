package com.wasimaster.wmpass.ui.parts

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.VisibilityThreshold
import androidx.compose.animation.core.spring
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.unit.IntSize
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion

/**
 * Places the old list's leavers among the new list's items, each where it was, so a chip on its
 * way out does not jump to the end before it goes.
 */
internal fun <T> mergeLeaving(before: List<T>, now: List<T>): List<T> {
    val out = ArrayList<T>(before.size + now.size)
    val placed = HashSet<T>()
    var next = 0
    for (item in before) {
        val at = now.indexOf(item)
        if (at < 0) {
            if (placed.add(item)) out += item
            continue
        }
        while (next <= at) {
            if (placed.add(now[next])) out += now[next]
            next++
        }
    }
    while (next < now.size) {
        if (placed.add(now[next])) out += now[next]
        next++
    }
    return out
}

/**
 * Each of [items], growing in when it arrives and shrinking away when it leaves, for chips in a
 * flowing row. The neighbours slide over as one widens or narrows, instead of jumping. Items are
 * their own keys, so they must be distinct.
 */
@Composable
fun <T> AppearingEach(items: List<T>, content: @Composable (T) -> Unit) {
    if (LocalReduceMotion.current) {
        items.forEach { item -> key(item) { content(item) } }
        return
    }
    // What showed at first is there already, and comes in without a flourish.
    val states = remember { HashMap<T, MutableTransitionState<Boolean>>().apply { items.forEach { put(it, MutableTransitionState(true)) } } }
    var shown by remember { mutableStateOf(items) }
    val merged = mergeLeaving(shown, items)
    SideEffect { if (merged != shown) shown = merged }
    merged.forEach { item ->
        key(item) {
            val state = states.getOrPut(item) { MutableTransitionState(false) }
            val wanted = item in items
            SideEffect { state.targetState = wanted }
            AnimatedVisibility(
                state,
                enter = fadeIn(spring(stiffness = 500f)) +
                    scaleIn(spring(dampingRatio = 0.7f, stiffness = 500f), initialScale = 0.6f) +
                    expandHorizontally(spring(dampingRatio = 0.9f, stiffness = 500f, visibilityThreshold = IntSize.VisibilityThreshold), expandFrom = Alignment.Start),
                exit = fadeOut(spring(stiffness = 700f)) +
                    scaleOut(spring(stiffness = 700f), targetScale = 0.6f) +
                    shrinkHorizontally(spring(dampingRatio = 0.9f, stiffness = 600f, visibilityThreshold = IntSize.VisibilityThreshold), shrinkTowards = Alignment.Start),
            ) {
                content(item)
            }
            // Gone for good once it has finished leaving.
            if (!wanted && state.isIdle && !state.currentState) {
                LaunchedEffect(Unit) {
                    states.remove(item)
                    shown = shown - item
                }
            }
        }
    }
}

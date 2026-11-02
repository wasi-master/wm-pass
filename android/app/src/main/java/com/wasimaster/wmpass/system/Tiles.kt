package com.wasimaster.wmpass.system

import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** Quick settings: lights up while the vault is open, and a tap shuts it. */
class LockTile : TileService() {
    private val scope = CoroutineScope(Dispatchers.Main.immediate)

    override fun onStartListening() {
        super.onStartListening()
        refresh()
    }

    private fun refresh() {
        val tile = qsTile ?: return
        tile.state = if (Pass.state.value is LockState.Unlocked) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.updateTile()
    }

    override fun onClick() {
        super.onClick()
        scope.launch {
            Pass.lock()
            refresh()
        }
    }
}

/**
 * A panic switch. It locks at once and refuses every password for an hour, so that it works
 * even from a phone taken while unlocked. It is always available: it needs no vault to be open.
 */
class PanicTile : TileService() {
    private val scope = CoroutineScope(Dispatchers.Main.immediate)

    override fun onClick() {
        super.onClick()
        scope.launch { Pass.panic() }
    }
}

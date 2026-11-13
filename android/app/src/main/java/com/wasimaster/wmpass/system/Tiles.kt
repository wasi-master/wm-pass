package com.wasimaster.wmpass.system

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/** Quick settings: lights up while the vault is open, and a tap shuts it. Locked, a tap opens it. */
class LockTile : TileService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var watching: Job? = null

    // Follows the vault while the panel is open, so a lock or an unlock meanwhile shows at once.
    override fun onStartListening() {
        super.onStartListening()
        watching?.cancel()
        watching = scope.launch { Pass.state.collect { refresh() } }
    }

    override fun onStopListening() {
        watching?.cancel()
        watching = null
        super.onStopListening()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun refresh() {
        val tile = qsTile ?: return
        val state = Pass.state.value
        tile.state = when (state) {
            is LockState.Unlocked -> Tile.STATE_ACTIVE
            LockState.Locked -> Tile.STATE_INACTIVE
            // No vault to lock: greyed out, not a switch that seems to do nothing.
            LockState.NoVault -> Tile.STATE_UNAVAILABLE
        }
        // Said in words too: on and off alone do not say which of the two is locked.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            tile.subtitle = when (state) {
                is LockState.Unlocked -> "Open"
                LockState.Locked -> "Locked"
                LockState.NoVault -> "No vault"
            }
        }
        tile.updateTile()
    }

    override fun onClick() {
        super.onClick()
        if (Pass.state.value == LockState.Locked) {
            // A switch that did nothing when off: it opens the vault, over whatever is on screen.
            if (isLocked) unlockAndRun { openDoor() } else openDoor()
            return
        }
        scope.launch {
            runCatching { Pass.lock() }
            refresh()
        }
    }

    private fun openDoor() {
        val intent = Intent(this, UnlockActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }
}

/**
 * A panic switch. It locks at once and refuses every password for an hour, so that it works
 * even from a phone taken while unlocked. It is always available: it needs no vault to be open.
 */
class PanicTile : TileService() {
    // Never cancelled with the tile: a panic cut off after the lock would skip the hour's refusal.
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun onClick() {
        super.onClick()
        scope.launch { runCatching { Pass.panic() } }
    }
}

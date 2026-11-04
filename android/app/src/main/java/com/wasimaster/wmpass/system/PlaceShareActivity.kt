package com.wasimaster.wmpass.system

import android.content.Intent
import android.os.Bundle
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.settings.placeIcon
import com.wasimaster.wmpass.ui.settings.placeLabel
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette

/**
 * WM Keyboard's "Use a place from WM Pass": the person unlocks, picks one of WM Pass's places for
 * copies, and WM Keyboard gets it back as a [PlaceWire] result, login included.
 *
 * Exported, since WM Keyboard has to start it, so it answers only the real WM Keyboard, by its
 * signing certificate; anything else, or a start that does not wait for a result, gets nothing and
 * is told nothing. The places are sealed on this phone and could be read without the vault, but
 * this screen still waits behind the door: a place's password leaving WM Pass is the owner's call,
 * made here with the vault open and a tap on that one place.
 */
class PlaceShareActivity : SecureActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // [getCallingPackage] is set only for a screen started for a result, the one way back.
        if (!KeyboardProvider.isKeyboard(this, callingPackage)) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }
        gated { Pick() }
    }

    private fun give(place: Destination) {
        val wire = PlaceWire.encode(place) ?: return
        setResult(RESULT_OK, Intent().putExtra(PlaceWire.EXTRA, wire))
        finish()
    }

    @Composable
    private fun Pick() {
        val colors = palette
        val all by Destinations.list.collectAsStateWithLifecycle()
        val places = all.filter { PlaceWire.encode(it) != null }
        Column(Modifier.fillMaxSize().background(colors.ground).statusBarsPadding().navigationBarsPadding()) {
            Text(
                "Give WM Keyboard a place",
                color = colors.text,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
            )
            Text(
                if (places.isEmpty()) {
                    "There are no places for copies here yet."
                } else {
                    "WM Keyboard gets its own copy of the place you tap, with its login. An account you sign in to " +
                        "(Google Drive, Dropbox, OneDrive, Box, pCloud, MEGA, iCloud Drive, Proton Drive) goes without " +
                        "the sign-in, and a folder on this phone goes without the folder: WM Keyboard asks for those again."
                },
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 20.dp),
            )
            LazyColumn(
                Modifier.weight(1f).padding(top = 16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(places, key = { it.id }) { place -> PlaceRow(place) { give(place) } }
            }
            QuietButton("Cancel", Modifier.fillMaxWidth().padding(16.dp)) { finish() }
        }
    }

    @Composable
    private fun PlaceRow(place: Destination, onClick: () -> Unit) {
        val colors = palette
        Row(
            Modifier
                .padding(horizontal = 16.dp)
                .fillMaxWidth()
                .clip(Shapes.cardSmall)
                .background(colors.surface1)
                .rim(Shapes.cardSmall, colors)
                .clickable(onClick = onClick)
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphTile(placeIcon(place.kind), colors.primary, size = 40.dp)
            Spacer(Modifier.width(13.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    place.label,
                    color = colors.text,
                    style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(placeLabel(place.kind), color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
            }
        }
    }
}

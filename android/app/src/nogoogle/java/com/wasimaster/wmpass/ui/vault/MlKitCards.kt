package com.wasimaster.wmpass.ui.vault

/**
 * The F-Droid and offline builds carry no Google SDK, so there is no Google reader: the card
 * scanner uses WM Pass's own. src/google has the real one.
 */
internal object MlKitCards {
    fun engine(): CardEngine? = null

    /** What ML Kit alone reads in a picture: never, here. */
    fun lines(@Suppress("UNUSED_PARAMETER") context: android.content.Context, @Suppress("UNUSED_PARAMETER") picture: android.graphics.Bitmap): Pair<Array<String>, FloatArray>? = null
}

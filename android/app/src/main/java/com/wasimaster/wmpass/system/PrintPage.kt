package com.wasimaster.wmpass.system

import android.annotation.SuppressLint
import android.app.Activity
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintManager
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient

/**
 * Hands the printable page to Android's print screen, where it can go to a printer or be saved
 * as a PDF. The page is drawn in a WebView that runs no script and loads nothing from anywhere,
 * and the view is torn down once the print screen is done with it.
 */
object PrintPage {
    // A WebView held only by its print adapter can be collected before the adapter writes.
    @SuppressLint("StaticFieldLeak")
    private var held: WebView? = null

    fun print(activity: Activity, html: String, title: String) {
        held?.destroy()
        val web = WebView(activity)
        held = web
        web.settings.apply {
            javaScriptEnabled = false
            blockNetworkLoads = true
            allowFileAccess = false
            allowContentAccess = false
        }
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = true

            override fun onPageFinished(view: WebView, url: String?) {
                val inner = view.createPrintDocumentAdapter(title)
                val manager = activity.getSystemService(PrintManager::class.java)
                manager.print(title, Released(inner, view), PrintAttributes.Builder().setMediaSize(PrintAttributes.MediaSize.ISO_A4).build())
            }
        }
        web.loadDataWithBaseURL(null, html, "text/html", "utf-8", null)
    }

    /** Passes everything through, then lets the WebView go when the print screen closes. */
    private class Released(private val inner: PrintDocumentAdapter, private val web: WebView) : PrintDocumentAdapter() {
        override fun onStart() = inner.onStart()

        override fun onLayout(
            oldAttributes: PrintAttributes?,
            newAttributes: PrintAttributes,
            cancellationSignal: CancellationSignal?,
            callback: LayoutResultCallback,
            extras: Bundle?,
        ) = inner.onLayout(oldAttributes, newAttributes, cancellationSignal, callback, extras)

        override fun onWrite(
            pages: Array<out PageRange>,
            destination: ParcelFileDescriptor,
            cancellationSignal: CancellationSignal,
            callback: WriteResultCallback,
        ) = inner.onWrite(pages, destination, cancellationSignal, callback)

        override fun onFinish() {
            inner.onFinish()
            web.clearCache(true)
            web.destroy()
            if (held === web) held = null
        }
    }
}

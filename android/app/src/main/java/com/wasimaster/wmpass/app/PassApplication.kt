package com.wasimaster.wmpass.app

import android.app.Application
import com.wasimaster.wmpass.core.Pass

class PassApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        // First, so a look that keeps crashing is gone before anything reads it.
        LaunchGuard.start(this) { Prefs.of(this).resetLook() }
        Pass.init(this)
        SettingsSync.start(Prefs.of(this))
        AutoLock.install(this)
        // The watch asks this before each answer, as autofill and the keyboard check the timer.
        com.wasimaster.wmpass.system.WatchGate.install(this)
        Thread { com.wasimaster.wmpass.ui.parts.Brands.warm(this) }.start()
        // Panic apps chosen before their certificates were kept get them now, as installed at
        // this start, the same trust Settings would give them, so their panic signal keeps working.
        Thread { runCatching { com.wasimaster.wmpass.system.PanicPins.pinMissing(this, Prefs.of(this)) } }.start()
        // Only where to read it from: the guide is parsed the first time it is needed.
        com.wasimaster.wmpass.ui.help.Help.attach(this)
        com.wasimaster.wmpass.system.Accounts.load(this)
        com.wasimaster.wmpass.system.Destinations.load(this)
        // After the places, so the Backups widget has them; it redraws every widget locked first.
        com.wasimaster.wmpass.system.widgets.WidgetFeed.start(this)
        com.wasimaster.wmkeyboard.core.settings.sink.AutoBackupNaming.appFiles = com.wasimaster.wmpass.system.Backups::isOurs
        com.wasimaster.wmpass.system.CloudSignIn.install()
        com.wasimaster.wmsuite.sinks.net.SinkNet.checkInternet = {
            if (com.wasimaster.wmpass.BuildConfig.OFFLINE) throw java.io.IOException("This build has no network access.")
        }
        Pass.afterSave = {
            val lan = com.wasimaster.wmpass.system.Lan
            // A save after a sync on this network goes on to the sync places only if it brought something.
            if (!lan.savingOwn || lan.savingReceived) com.wasimaster.wmpass.system.FileSync.soon(this)
            lan.soon(this)
        }
        // Sync places and paired devices on this network keep up by themselves while the vault is open.
        com.wasimaster.wmpass.system.FileSync.startAuto(this)
        com.wasimaster.wmpass.system.Lan.startAuto(this)
        // A wipe takes what is kept beside the vault too: a duress password must not leave
        // the addresses of the backups and accounts behind.
        Pass.onWipe += {
            com.wasimaster.wmpass.system.Sealed.wipe(this)
            com.wasimaster.wmpass.system.Destinations.forget(this)
            com.wasimaster.wmpass.system.Accounts.forget(this)
            com.wasimaster.wmpass.system.widgets.WidgetFeed.forget(this)
            androidx.work.WorkManager.getInstance(this).cancelAllWork()
        }
    }
}

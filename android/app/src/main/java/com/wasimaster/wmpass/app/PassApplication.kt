package com.wasimaster.wmpass.app

import android.app.Application
import com.wasimaster.wmpass.core.Pass

class PassApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        // First, so a look that keeps crashing is gone before anything reads it.
        LaunchGuard.start(this) { Prefs.of(this).resetLook() }
        Pass.init(this)
        AutoLock.install(this)
        Thread { com.wasimaster.wmpass.ui.parts.Brands.warm(this) }.start()
        com.wasimaster.wmpass.system.Accounts.load(this)
        com.wasimaster.wmpass.system.Destinations.load(this)
        com.wasimaster.wmkeyboard.core.settings.sink.AutoBackupNaming.appFiles = com.wasimaster.wmpass.system.Backups::isOurs
        com.wasimaster.wmsuite.sinks.net.SinkNet.checkInternet = {
            if (com.wasimaster.wmpass.BuildConfig.OFFLINE) throw java.io.IOException("This build has no network access.")
        }
        Pass.afterSave = { com.wasimaster.wmpass.system.FileSync.soon(this) }
        // A wipe takes what is kept beside the vault too: a duress password must not leave
        // the addresses of the backups and accounts behind.
        Pass.onWipe += {
            com.wasimaster.wmpass.system.Sealed.wipe(this)
            com.wasimaster.wmpass.system.Destinations.forget()
            com.wasimaster.wmpass.system.Accounts.forget(this)
            androidx.work.WorkManager.getInstance(this).cancelAllWork()
        }
    }
}

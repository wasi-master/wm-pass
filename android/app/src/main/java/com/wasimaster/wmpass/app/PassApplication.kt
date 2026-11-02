package com.wasimaster.wmpass.app

import android.app.Application
import com.wasimaster.wmpass.core.Pass

class PassApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        Pass.init(this)
        AutoLock.install(this)
    }
}

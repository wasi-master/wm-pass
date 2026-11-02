package com.wasimaster.wmpass.ui.settings

import androidx.compose.runtime.Composable
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmsuite.settings.WmScreen

@Composable fun BreachScreen() = Blank("Breaches")
@Composable fun BackupScreen() = Blank("Backups")
@Composable fun SyncScreen() = Blank("Sync and accounts")
@Composable fun AliasScreen() = Blank("Email aliases")
@Composable fun UsernameScreen() = Blank("Username checker")
@Composable fun InboxScreen() = Blank("Disposable inbox")

@Composable
private fun Blank(title: String) {
    val shell = LocalShell.current
    WmScreen(title, onBack = { shell.nav.popBackStack() }) { }
}

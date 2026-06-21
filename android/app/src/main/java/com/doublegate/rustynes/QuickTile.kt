package com.doublegate.rustynes

import android.app.PendingIntent
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/**
 * v1.8.8 "Atlas" (Workstream H): a "Resume RustyNES" Quick Settings tile.
 *
 * Tapping it launches the app to the last-played ROM (via [DeepLink.ACTION_RESUME]);
 * the tile subtitle (API 29+) shows the game's name so the user sees what will
 * resume. Registered in the manifest with the BIND_QUICK_SETTINGS_TILE permission +
 * the QS_TILE intent-filter. The user adds it from the QS edit tray; the system
 * binds this service while the tile is visible.
 */
class ResumeTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        val tile = qsTile ?: return
        val last = DeepLink.lastPlayed(applicationContext)
        tile.state = if (last != null) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = getString(R.string.tile_resume_label)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            tile.subtitle = last?.name ?: getString(R.string.tile_resume_none)
        }
        tile.icon = Icon.createWithResource(this, R.drawable.ic_tile_resume)
        tile.updateTile()
    }

    override fun onClick() {
        super.onClick()
        val intent = DeepLink.intent(applicationContext, DeepLink.ACTION_RESUME)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // API 34+: startActivityAndCollapse takes a PendingIntent.
            val pi = PendingIntent.getActivity(
                this,
                0,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            startActivityAndCollapse(pi)
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(intent)
        }
    }
}

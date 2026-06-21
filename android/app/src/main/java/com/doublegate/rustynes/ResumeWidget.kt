package com.doublegate.rustynes

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Column
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.padding
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider

/**
 * v1.8.8 "Atlas" (Workstream H): a "Resume <last game>" home-screen widget, built
 * with Glance (Compose-idiomatic, since the whole app is Compose).
 *
 * It shows the last-played game's name and, on tap, launches the app straight into
 * it (the same [DeepLink.ACTION_RESUME] the Quick Settings tile uses). With nothing
 * played yet it falls back to a "RustyNES — open a game" prompt that opens the app.
 *
 * The "last game" is read from [GameLibrary] (the most-recently-played entry) inside
 * `provideGlance`, so the widget always reflects the live library without any extra
 * state plumbing. Presentation-only; the core/determinism contract is untouched.
 */
class ResumeWidget : GlanceAppWidget() {

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        // Read the live last-played entry off the library (suspend context).
        val last = DeepLink.lastPlayed(context)
        provideContent {
            GlanceTheme {
                WidgetContent(context, last)
            }
        }
    }

    @Composable
    private fun WidgetContent(context: Context, last: GameEntry?) {
        // Tap target: resume the last game, or open the app if none yet.
        val action = if (last != null) DeepLink.ACTION_RESUME else DeepLink.ACTION_LIBRARY
        Column(
            modifier = GlanceModifier
                .fillMaxSize()
                .background(Color(0xFF221C4A))
                .cornerRadius(16.dp)
                .padding(12.dp)
                .clickable(actionStartActivity(DeepLink.intent(context, action))),
            verticalAlignment = Alignment.Vertical.CenterVertically,
            horizontalAlignment = Alignment.Horizontal.Start,
        ) {
            Text(
                text = context.getString(R.string.widget_resume_title),
                style = TextStyle(
                    color = ColorProvider(Color(0xFFB7AEF5)),
                    fontWeight = FontWeight.Medium,
                    fontSize = 13.sp,
                ),
            )
            Text(
                text = last?.name ?: context.getString(R.string.widget_resume_none),
                style = TextStyle(
                    color = ColorProvider(Color.White),
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp,
                ),
                maxLines = 2,
            )
        }
    }
}

/** The system-facing receiver that binds [ResumeWidget] to the AppWidget host. */
class ResumeWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = ResumeWidget()
}

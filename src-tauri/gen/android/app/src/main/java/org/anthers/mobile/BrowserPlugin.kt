// SPDX-License-Identifier: Apache-2.0
package org.anthers.mobile

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.net.toUri
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

/**
 * Opens a URL in a browser, and deliberately not in this app.
 *
 * 🚨 **Leaving this app out of the candidates is the whole point.** The shell verifies
 * `https://anthers.org` app links so the OAuth callback can come back to it, which also makes
 * it a handler for every other anthers.org URL — including the authorize page the sign-in
 * hands to the browser. A plain `ACTION_VIEW` for that page resolves straight back into this
 * app, whose activity is `singleTask`, so the window simply stays where it is: the handoff
 * becomes a no-op with no error to report, which is exactly how it failed on a phone while
 * working in an emulator whose build is not a verified app-link handler.
 *
 * So the candidates are asked for, this package is removed from the list, and the browser that
 * remains is given the URL explicitly. `ACTION_VIEW` is still the action, so any installed
 * browser answers; a phone with none gets an error the sign-in card can show.
 */
@InvokeArg
internal class OpenArgs {
    lateinit var url: String
}

@TauriPlugin
class BrowserPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun open(invoke: Invoke) {
        try {
            val url = invoke.parseArgs(OpenArgs::class.java).url
            val view = Intent(Intent.ACTION_VIEW, url.toUri())
                .addCategory(Intent.CATEGORY_BROWSABLE)
            val browsers = activity.packageManager
                .queryIntentActivities(view, PackageManager.MATCH_DEFAULT_ONLY)
                .map { it.activityInfo.packageName }
                .filter { it != activity.packageName }
                .distinct()
            val target = browsers.firstOrNull()
                ?: throw IllegalStateException("No browser is installed to open the sign-in.")

            view.setPackage(target)
            view.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.applicationContext.startActivity(view)
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject(ex.message)
        }
    }
}

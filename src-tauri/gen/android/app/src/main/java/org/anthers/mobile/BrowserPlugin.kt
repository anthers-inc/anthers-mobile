// SPDX-License-Identifier: Apache-2.0
package org.anthers.mobile

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import androidx.browser.customtabs.CustomTabsClient
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
            val uri = url.toUri()

            // 🚨 **Name the browser, do not describe the URL.** A browser registers a
            // `CustomTabsService` to say it can render pages, and asking for that service is a
            // different lookup from resolving an `ACTION_VIEW` against installed URL filters —
            // which is the one that came back empty on a phone whose browser opens the same URL
            // happily when it is pasted. With a package in hand the intent is explicit, so no
            // matching happens at all.
            val browser = CustomTabsClient.getPackageName(activity, null)
            if (browser != null) {
                val explicit = Intent(Intent.ACTION_VIEW, uri)
                    .addCategory(Intent.CATEGORY_BROWSABLE)
                    .setPackage(browser)
                explicit.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                activity.applicationContext.startActivity(explicit)
                invoke.resolve()
                return
            }

            // No browser offers Custom Tabs: fall back to whatever the system matches, which is
            // every other browser.
            val view = Intent(Intent.ACTION_VIEW, uri)
                .addCategory(Intent.CATEGORY_BROWSABLE)

            // The direct path: every visible handler for this URL except this app. It is empty
            // either when no browser is installed or, more often, when Android's package
            // visibility hides them from us — the `<queries>` block in the manifest is what
            // makes them visible, and a query that disagrees with it by so much as a category
            // comes back empty on a phone that has three browsers.
            val browsers = activity.packageManager
                .queryIntentActivities(view, PackageManager.MATCH_DEFAULT_ONLY)
                .map { it.activityInfo.packageName }
                .filter { it != activity.packageName }
                .distinct()

            if (browsers.isNotEmpty()) {
                view.setPackage(browsers.first())
                view.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                activity.applicationContext.startActivity(view)
                invoke.resolve()
                return
            }

            // 🚨 **The fallback is a chooser, not an error.** The system resolves a chooser
            // itself, so it needs no visibility of its own, and excluding this activity keeps
            // the app from being one of the answers — which is the whole point of opening a
            // URL it is a verified handler for.
            val chooser = Intent.createChooser(view, null)
            chooser.putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, arrayOf(activity.componentName))
            chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.applicationContext.startActivity(chooser)
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject(ex.message)
        }
    }
}

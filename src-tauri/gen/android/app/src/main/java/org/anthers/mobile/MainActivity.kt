package org.anthers.mobile

import android.os.Bundle
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.roundToInt

/**
 * The window, and the insets the web page cannot see.
 *
 * `enableEdgeToEdge()` plus a target SDK past 35 means the WebView spans the whole screen,
 * under the status bar, the gesture/three-button navigation bar and any camera cutout — and
 * Android 15 and later enforce that whether or not the call is present. The web app claims
 * that space back with CSS, but an Android WebView does not report the system bars through
 * `env(safe-area-inset-*)`: at the WebView versions this app supports, `env()` carries only
 * the display cutout, and the status and navigation bars are invisible to CSS.
 *
 * So the shell measures the real window insets and publishes them to the page:
 *
 * - `__ANTHERS_INSETS__.get()` answers the latest values as JSON in **CSS pixels**, which is
 *   the unit the stylesheet consumes. Dividing by `displayMetrics.density` here is what keeps
 *   the padding right across screen densities.
 * - `__ANTHERS_APPLY_INSETS__(payload)` is defined by the injected init script in `lib.rs`
 *   and writes the values to the document's custom properties. A push can land before the
 *   page's document exists, so the init script also pulls once on `DOMContentLoaded`; that
 *   pull is what makes a cold start correct rather than dependent on timing.
 * - `systemBars() or displayCutout()` and the max of the two per edge: the status bar is not
 *   always the deepest part of the top edge on a phone with a punch-hole camera, and the
 *   gesture bar is not always the deepest part of the bottom.
 *
 * ⚠️ **The listener returns the insets unconsumed.** Consuming them here would be this view
 * claiming the WebView's own display-cutout handling as well, and the page's CSS is the layer
 * that decides where the chrome goes.
 */
class MainActivity : TauriActivity() {
  private data class Insets(
    val top: Int = 0,
    val right: Int = 0,
    val bottom: Int = 0,
    val left: Int = 0,
  ) {
    fun json(): String = """{"top":$top,"right":$right,"bottom":$bottom,"left":$left}"""
  }

  private var webView: WebView? = null
  // Read by the JS bridge on its own thread, written on the UI thread when the insets change.
  @Volatile private var insets = Insets()

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { _, windowInsets ->
      val bars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val density = resources.displayMetrics.density
      val next = Insets(
        top = (bars.top / density).roundToInt(),
        right = (bars.right / density).roundToInt(),
        bottom = (bars.bottom / density).roundToInt(),
        left = (bars.left / density).roundToInt(),
      )
      if (next != insets) {
        insets = next
        webView?.let(::publish)
      }
      windowInsets
    }
    // The first dispatch can happen while the view is being attached, before the listener
    // above exists; ask for a pass now rather than depending on a later change.
    ViewCompat.requestApplyInsets(content)
  }

  /**
   * The WebView is handed over here before it loads, which is the one moment an interface
   * added with `addJavascriptInterface` is guaranteed to exist for the page's init scripts.
   */
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    this.webView = webView
    webView.addJavascriptInterface(Bridge(), "__ANTHERS_INSETS__")
    publish(webView)
  }

  /**
   * Hand the current values to the page. The function belongs to the injected init script;
   * when it is not there yet (the document is still the blank one), the page's own pull on
   * `DOMContentLoaded` carries the same values, so a missed push is not a missed inset.
   */
  private fun publish(webView: WebView) {
    val script =
      "window.__ANTHERS_APPLY_INSETS__ && window.__ANTHERS_APPLY_INSETS__(${insets.json()});"
    webView.post { webView.evaluateJavascript(script, null) }
  }

  /** Read-only, and only the four numbers — there is nothing here worth exposing. */
  inner class Bridge {
    @JavascriptInterface
    fun get(): String = insets.json()
  }
}

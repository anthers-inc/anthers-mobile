// SPDX-License-Identifier: Apache-2.0

//! Opening a URL in a browser.
//!
//! 🚨 **Not `tauri-plugin-opener`'s mobile path, and the reason is a collision inside Android's
//! intent resolution rather than a defect in that plugin.** The shell registers verified
//! `https://anthers.org` app links so the OAuth callback can return to it, which makes it a
//! handler for every other anthers.org URL too — so the plugin's plain `ACTION_VIEW` for the
//! authorize page resolved back into this app and the sign-in handoff did nothing at all, with
//! no error to show. The Kotlin side here asks for the URL's handlers, drops this app, and
//! gives the browser that remains an explicit intent.
//!
//! On the desktop there is no such collision and the plugin's own opener is right.

/// The Android plugin handle the Kotlin side is reached through.
#[cfg(target_os = "android")]
pub struct Browser(pub tauri::plugin::PluginHandle<tauri::Wry>);

/// Nothing to hold where the desktop's opener is reached directly.
#[cfg(not(target_os = "android"))]
pub struct Browser;

impl Browser {
	/// Open `url` in the system browser. A phone with no browser answers with an error the
	/// caller can show, rather than silently doing nothing.
	pub fn open(&self, url: &str) -> Result<(), String> {
		#[cfg(target_os = "android")]
		{
			self.0
				.run_mobile_plugin::<()>("open", serde_json::json!({ "url": url }))
				.map_err(|e| e.to_string())
		}
		#[cfg(not(target_os = "android"))]
		{
			tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
		}
	}
}

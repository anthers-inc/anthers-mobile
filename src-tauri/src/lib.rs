// SPDX-License-Identifier: Apache-2.0

//! Anthers — the mobile shell around the whole Anthers app.
//!
//! It bundles the SAME built web app the browser and the desktop shell serve, so the whole
//! platform travels with the app — reading, watching and creating. This is a consumer of
//! `@anthers/web-shared`, not a fork, and it starts as a one-to-one wrapper: what the shell
//! adds is what a browser tab cannot do, and nothing else.
//!
//! ## Why the app is not just pointed at anthers.org
//!
//! `invoke` is unavailable to remote documents, so native capability would mean opening the
//! IPC boundary to a page served over the internet — an XSS on the site would then reach the
//! device. The SPA is bundled instead. The desktop shell settled this first; the README
//! carries the reasoning.
//!
//! ## Why the webview needs the runtime injected
//!
//! A bundled webview serves from `http://tauri.localhost` on Android (and `tauri://localhost`
//! on iOS), where the API origin cannot be sniffed from the document's own host and the
//! `.anthers.org` session cookie is never sent. `apiBaseUrl()` in `@anthers/web-shared` reads
//! `globalThis.__ANTHERS_DESKTOP__` for exactly this case, so the shell defines it before any
//! app JS runs — the plugin's `js_init_script` is what guarantees that ordering, on every
//! platform, without depending on when a window happens to exist.
//!
//! ⚠️ **The seam is still named for the desktop, and this shell borrows it.** Renaming it to
//! a platform-tagged runtime (`__ANTHERS_NATIVE__` carrying `platform: "android" | "ios" |
//! "desktop"`) is a platform-repository change, and it is the first thing the mobile work
//! needs there: until it lands, `isDesktop()` answers true inside this shell, so any
//! desktop-only branch in the web app will run on a phone.
//!
//! ## What this first pass does not have
//!
//! No token and therefore no sign-in: `getToken()` returns null and the app browses what a
//! signed-out reader sees. The keystore-held token, the system-browser handoff and verified
//! app links are the lane's later milestones.

/// The API origin this build talks to. Override at build time:
/// `ANTHERS_API_BASE=https://staging.example.org bunx tauri android build`.
///
/// Debug builds point at the Android emulator's host alias rather than `localhost`, which on
/// a phone is the phone itself. A physical device needs the LAN address of the machine running
/// `make dev`, passed through `ANTHERS_API_BASE`.
fn api_base_url() -> String {
	option_env!("ANTHERS_API_BASE")
		.map(str::to_string)
		.unwrap_or_else(|| {
			if cfg!(debug_assertions) {
				"http://10.0.2.2:8000".to_string()
			} else {
				"https://anthers.org".to_string()
			}
		})
}

/// The runtime `apiBaseUrl()` reads, defined before any app JS in every webview.
///
/// `fetch` is deliberately not supplied, matching the desktop: `apiFetch()` then uses the
/// webview's own, a non-credentialed cross-origin request carrying no cookie, which the API's
/// origin allow-list admits (`http://tauri.localhost` is the same origin Windows produces).
fn runtime_script(api_base: &str) -> String {
	let base = serde_json::to_string(api_base).unwrap_or_else(|_| "\"\"".into());
	format!(
		r#"(() => {{
	globalThis.__ANTHERS_DESKTOP__ = {{
		apiBaseUrl: {base},
		getToken: () => null,
	}};
}})();"#
	)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
	let script = runtime_script(&api_base_url());

	tauri::Builder::default()
		// Registered before the runtime plugin so a link that launched the app is already
		// available when the webview's script runs.
		.plugin(tauri_plugin_deep_link::init())
		.plugin(
			tauri::plugin::Builder::<tauri::Wry>::new("anthers-runtime")
				.js_init_script(script)
				.build(),
		)
		.setup(|app| {
			// A link that opens the app while it is running arrives here. Android writes
			// this line to logcat, which is the only place a delivery can be observed
			// without a person watching the screen — the sign-in flow reads the same event
			// for its callback code.
			use tauri_plugin_deep_link::DeepLinkExt;
			app.deep_link().on_open_url(|event| {
				for url in event.urls() {
					eprintln!("[anthers] deep link: {url}");
				}
			});
			Ok(())
		})
		.run(tauri::generate_context!())
		.expect("error while running the Anthers mobile shell");
}

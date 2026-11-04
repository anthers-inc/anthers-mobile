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
//! device. The SPA is bundled instead. The desktop shell settled this first.
//!
//! ## Why the webview needs the runtime injected
//!
//! A bundled webview serves from `http://tauri.localhost` on Android, where the API origin
//! cannot be sniffed from the document's own host and the `.anthers.org` session cookie is
//! never sent. `apiBaseUrl()` in `@anthers/web-shared` reads `globalThis.__ANTHERS_NATIVE__`
//! for exactly this case, so the shell defines it before any app JS runs — the plugin's
//! `js_init_script` is what guarantees that ordering on every platform.
//!
//! ⚠️ **The token arrives a moment later than the shape does.** A stored session has to be
//! read from the Android keystore, which is a blocking call that cannot run on the main
//! thread, so `on_webview_ready` reads it on its own thread and evaluates `setToken` into the
//! page. `getToken()` answers null until then; `apiFetch` re-roots per request, so the
//! requests the app makes after its first paint carry the session.

mod auth;
mod browser;
mod keystore;

use tauri::{Emitter, Manager};

use auth::PendingCode;
use browser::Browser;
use keystore::Keystore;

/// The API origin this build talks to. Override at build time:
/// `ANTHERS_API_BASE=https://staging.example.org bunx tauri android build`.
///
/// Debug builds point at the Android emulator's host alias rather than `localhost`, which on
/// a phone is the phone itself.
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

/// The shape `apiBaseUrl()` reads, defined before any app JS in every webview.
///
/// `fetch` is deliberately not supplied, matching the desktop: `apiFetch()` then uses the
/// webview's own, a non-credentialed cross-origin request carrying no cookie, which the API's
/// origin allow-list admits (`http://tauri.localhost` is the same origin Windows produces).
///
/// It also carries the safe-area bridge. The Android WebView does not report the status bar
/// or the navigation bar through `env(safe-area-inset-*)`; `MainActivity` measures the real
/// window insets and exposes them on `__ANTHERS_INSETS__`, and this script is what turns the
/// numbers into the four custom properties the web app's CSS reads. The pull on
/// `DOMContentLoaded` is load-bearing: a native push can land while the document is still the
/// blank one, and a page load discards whatever that document's style said.
fn runtime_script(api_base: &str) -> String {
	let base = serde_json::to_string(api_base).unwrap_or_else(|_| "\"\"".into());
	// `replace` rather than a `format!` string: the script's own braces would all need
	// doubling, and a JS template literal is unreadable enough without that.
	r#"(() => {
	let token = null;
	globalThis.__ANTHERS_NATIVE__ = {
		platform: "android",
		apiBaseUrl: __API_BASE__,
		getToken: () => token,
		setToken: (next) => { token = next; },
	};

	const applyInsets = (payload) => {
		const style = document.documentElement && document.documentElement.style;
		if (!style) return;
		let insets = payload;
		if (!insets) {
			const bridge = globalThis.__ANTHERS_INSETS__;
			if (!bridge) return;
			try {
				insets = JSON.parse(bridge.get());
			} catch {
				return;
			}
		}
		for (const edge of ["top", "right", "bottom", "left"]) {
			const value = Number(insets[edge]);
			if (Number.isFinite(value)) {
				style.setProperty(`--anthers-inset-${edge}`, `${value}px`);
			}
		}
	};
	globalThis.__ANTHERS_APPLY_INSETS__ = applyInsets;
	if (document.readyState === "loading") {
		document.addEventListener("DOMContentLoaded", () => applyInsets());
	} else {
		applyInsets();
	}
	window.addEventListener("resize", () => applyInsets());
})();"#
		.replace("__API_BASE__", &base)
}

/// Fill the token the webview will present, once the keystore has answered.
///
/// 🚨 **On its own thread, because the keystore call blocks.** It posts to the Android main
/// looper and waits on a channel, so running it on the main thread deadlocks the app before
/// its first frame. The `eval` is marshalled back to the main thread, which is where the
/// webview lives.
fn restore_session(webview: tauri::Webview<tauri::Wry>) {
	let app = webview.app_handle().clone();
	std::thread::spawn(move || {
		let Some(store) = app.try_state::<Keystore>() else {
			return;
		};
		#[cfg(all(target_os = "android", debug_assertions))]
		eprintln!(
			"[anthers] keystore selftest: {}",
			if store.selftest() { "ok" } else { "FAILED" }
		);
		let token = store.read();
		#[cfg(all(target_os = "android", debug_assertions))]
		eprintln!(
			"[anthers] stored session: {}",
			if token.is_some() { "present" } else { "none" }
		);
		let Some(token) = token else {
			return;
		};
		let script = match serde_json::to_string(&token) {
			Ok(literal) => format!("globalThis.__ANTHERS_NATIVE__?.setToken({literal});"),
			Err(_) => return,
		};
		let webview = webview.clone();
		let _ = app.run_on_main_thread(move || {
			let _ = webview.eval(&script);
		});
	});
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
	let script = runtime_script(&api_base_url());

	tauri::Builder::default()
		// The keystore plugin registers the Kotlin side and keeps its handle. Nothing in the
		// webview can reach it: only this process stores or clears a session.
		.plugin(
			tauri::plugin::Builder::<tauri::Wry>::new("anthers-keystore")
				.setup(|app, api| {
					#[cfg(target_os = "android")]
					{
						let handle =
							api.register_android_plugin("org.anthers.mobile", "KeystorePlugin")?;
						app.manage(Keystore(handle));
					}
					#[cfg(not(target_os = "android"))]
					{
						let _ = api;
						app.manage(Keystore);
					}
					Ok(())
				})
				.on_webview_ready(restore_session)
				.build(),
		)
		// The browser handoff. Its own plugin rather than `tauri-plugin-opener`, because the
		// verified `anthers.org` app link makes this app a candidate handler for the very URL
		// the sign-in opens; `browser.rs` carries the whole story.
		.plugin(
			tauri::plugin::Builder::<tauri::Wry>::new("anthers-browser")
				.setup(|app, api| {
					#[cfg(target_os = "android")]
					{
						let handle =
							api.register_android_plugin("org.anthers.mobile", "BrowserPlugin")?;
						app.manage(Browser(handle));
					}
					#[cfg(not(target_os = "android"))]
					{
						let _ = api;
						app.manage(Browser);
					}
					Ok(())
				})
				.build(),
		)
		.plugin(tauri_plugin_opener::init())
		.plugin(tauri_plugin_http::init())
		// Registered before the runtime plugin so a link that launched the app is already
		// available when the webview's script runs.
		.plugin(tauri_plugin_deep_link::init())
		.plugin(
			tauri::plugin::Builder::<tauri::Wry>::new("anthers-runtime")
				.js_init_script(script)
				.build(),
		)
		.setup(|app| {
			app.manage(PendingCode::default());

			// A link that opens the app while it is running arrives here. The code is kept for
			// the webview to pull on boot AND emitted, because the two cases are different: a
			// cold start has no listener yet, a running app does.
			// 🚨 **The phone opens the app on the callback link and then ignores the code in it.**
			// A one-time code is burned by the first redemption attempt against it, wrong
			// verifier or not, and the phone has two redeemers: the link this event carries, and
			// the poll the sign-in screen runs. The poll is the one that works — it survives a
			// browser that never hands off and a process Android reclaimed — so on Android the
			// link's only job is to bring the app forward, and the poll finishes the sign-in.
			// The desktop has no poll and keeps redeeming from its `anthers://` link.
			use tauri_plugin_deep_link::DeepLinkExt;
			let handle = app.handle().clone();
			app.deep_link().on_open_url(move |event| {
				for url in event.urls() {
					#[cfg(target_os = "android")]
					{
						eprintln!("[anthers] sign-in callback opened the app: {url}");
					}
					#[cfg(not(target_os = "android"))]
					{
						let code = auth::callback_code(&url);
						if let Some(code) = code {
							if let Some(pending) = handle.try_state::<PendingCode>() {
								pending.put(code.clone());
							}
							let _ = handle.emit("desktop-auth-code", code);
						} else {
							eprintln!("[anthers] deep link: {url}");
						}
					}
				}
			});
			Ok(())
		})
		.invoke_handler(tauri::generate_handler![
			auth::begin_sign_in,
			auth::await_sign_in,
			auth::has_pending_sign_in,
			auth::complete_sign_in,
			auth::take_pending_code,
			auth::sign_out
		])
		.run(tauri::generate_context!())
		.expect("error while running the Anthers mobile shell");
}

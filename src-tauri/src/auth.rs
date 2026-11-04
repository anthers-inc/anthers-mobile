// SPDX-License-Identifier: Apache-2.0

//! Signing in, the shell's half.
//!
//! The protocol is the desktop's, because the platform already speaks it and a second one
//! would be a second thing to keep true. `POST /api/auth/desktop/start` takes a PKCE
//! challenge; the person confirms in the **system browser**, at whichever server holds their
//! identity, because a password typed into this webview would be a second host asking for a
//! credential it is not the authority on; the browser hands back a one-time code, and
//! `POST /api/auth/desktop/exchange` trades code + verifier for a session token.
//!
//! 🚨 **The verifier never leaves this process.** That is what makes a stolen callback code
//! useless: on Android any app may claim a custom scheme, so the callback rides a verified
//! `https://anthers.org` link rather than `anthers://`, and the code alone does not redeem.
//!
//! ⚠️ **The command names are the desktop's** (`begin_sign_in`, `complete_sign_in`,
//! `take_pending_code`, `sign_out`) and the event is `desktop-auth-code`, because
//! `@anthers/web-shared`'s `desktop.ts` already invokes exactly those. Matching them is why
//! the sign-in surface needed no change to run on a phone.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

use crate::browser::Browser;
use crate::keystore::Keystore;

/// What the shell needs to finish a sign-in it started: the verifier it must prove, and the
/// challenge the server knows the enrollment by.
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
	verifier: String,
	challenge: String,
}

/// Where the sign-in in flight is kept.
///
/// 🚨 **On disk, not in memory, because the process is expected to die mid-flow.** The handoff
/// sends the person to a browser, and Android is free to reclaim this app while they are there;
/// state that lived only in a `Mutex` was gone by the time the sign-in finished, which is why a
/// completed sign-in ended at "This sign-in didn't start here". The app's private data directory
/// is the same place the session token's encrypted copy lives; this value is weaker than that
/// one and worth less: it expires with the challenge and is useless without the code.
fn pending_file(app: &AppHandle) -> Result<PathBuf, String> {
	app.path()
		.app_data_dir()
		.map(|dir| dir.join("sign-in-pending.json"))
		.map_err(|e| format!("Could not reach the app's data directory: {e}"))
}

fn remember_pending(app: &AppHandle, pending: &Pending) -> Result<(), String> {
	let path = pending_file(app)?;
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir).map_err(|e| format!("Could not keep the sign-in: {e}"))?;
	}
	let body =
		serde_json::to_string(pending).map_err(|e| format!("Could not keep the sign-in: {e}"))?;
	std::fs::write(&path, body).map_err(|e| format!("Could not keep the sign-in: {e}"))
}

/// What this app is waiting to redeem, read without consuming it, so a poll can retry.
fn read_pending(app: &AppHandle) -> Option<Pending> {
	let path = pending_file(app).ok()?;
	let body = std::fs::read_to_string(&path).ok()?;
	serde_json::from_str(&body).ok()
}

/// Forget the sign-in, once its code has been redeemed.
///
/// ⚠️ **Called after a successful exchange, never before it.** Dropping the record first would
/// make a transient network failure at the exchange unrecoverable — the person would be signed
/// in at their identity server and have to start the whole ceremony again.
fn forget_pending(app: &AppHandle) {
	if let Ok(path) = pending_file(app) {
		let _ = std::fs::remove_file(path);
	}
}

/// A callback code that arrived before the webview was listening — the link that launched
/// the app, most often. The webview pulls it once on boot.
#[derive(Default)]
pub struct PendingCode(Mutex<Option<String>>);

impl PendingCode {
	/// Keep a code for the webview to pull on boot. Later arrivals replace earlier ones: only
	/// the newest can still be redeemed.
	pub fn put(&self, code: String) {
		if let Ok(mut slot) = self.0.lock() {
			*slot = Some(code);
		}
	}
}

#[derive(Serialize)]
pub struct SignInResult {
	pub token: String,
	/// The label the web app already knows: a store that outlives the process. "memoryOnly"
	/// is what the desktop reports when no keychain is available; there is no equivalent
	/// here, because a phone always has a keystore.
	pub persistence: Persistence,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Persistence {
	Keychain,
}

/// Where the authorize page lives — the CONSUMER SITE's origin, which is not the API's.
///
/// In production they are the same apex origin; in development they are not, and building
/// this URL from the API base opens a route the API does not serve.
fn web_base_url() -> String {
	option_env!("ANTHERS_WEB_BASE")
		.map(str::to_string)
		.unwrap_or_else(|| {
			if cfg!(debug_assertions) {
				"http://10.0.2.2:3000".to_string()
			} else {
				"https://anthers.org".to_string()
			}
		})
}

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

fn random_hex() -> String {
	let mut bytes = [0u8; 32];
	getrandom::getrandom(&mut bytes).expect("the OS has no randomness");
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(input: &str) -> String {
	let digest = Sha256::digest(input.as_bytes());
	digest.iter().map(|b| format!("{b:02x}")).collect()
}

async fn post_json(
	url: &str,
	body: &serde_json::Value,
) -> Result<tauri_plugin_http::reqwest::Response, String> {
	tauri_plugin_http::reqwest::Client::new()
		.post(url)
		.header("Content-Type", "application/json")
		.body(body.to_string())
		.send()
		.await
		.map_err(|e| format!("Could not reach Anthers: {e}"))
}

async fn get_json(url: &str) -> Result<tauri_plugin_http::reqwest::Response, String> {
	tauri_plugin_http::reqwest::Client::new()
		.get(url)
		.send()
		.await
		.map_err(|e| format!("Could not reach Anthers: {e}"))
}

/// Mint a PKCE challenge, register it with the server, and open the authorize page in the
/// system browser. Returns the opened URL so the UI can offer it as copyable text — a button
/// that silently does nothing on a device with no browser handler is a dead end.
#[tauri::command]
pub async fn begin_sign_in(
	app: AppHandle,
	browser: State<'_, Browser>,
	handle: String,
	label: String,
) -> Result<String, String> {
	let verifier = random_hex();
	let challenge = sha256_hex(&verifier);

	let res = post_json(
		&format!("{}/api/auth/desktop/start", api_base_url()),
		&serde_json::json!({ "challenge": challenge, "label": label }),
	)
	.await?;
	if !res.status().is_success() {
		return Err(format!(
			"Anthers refused the sign-in request ({})",
			res.status()
		));
	}

	// Only keep the sign-in once the server has accepted its challenge, so a failed start
	// cannot leave a stale verifier a later redemption would try to spend.
	remember_pending(
		&app,
		&Pending {
			verifier: verifier.clone(),
			challenge: challenge.clone(),
		},
	)?;

	// `redirect=mobile` is what tells the page to answer on the app link rather than on the
	// desktop's custom scheme, and `handle` is what lets a browser with no Anthers session
	// start the OAuth at the person's own identity server instead of bouncing to `/login`.
	// See `DesktopAuthorizePage` in the platform repository.
	let url = format!(
		"{}/desktop/authorize?challenge={challenge}&redirect=mobile&handle={handle}",
		web_base_url()
	);
	// 🚨 **`Browser`, never a plain `ACTION_VIEW`.** This app answers verified `anthers.org`
	// links, so the authorize page it is handing over is a URL this app is a handler for: an
	// ordinary view intent resolves back into it, the window stays put, and the person has to
	// copy the link out by hand. `browser.rs` excludes this package before choosing.
	browser
		.open(&url)
		.map_err(|e| format!("Could not open your browser: {e}"))?;
	Ok(url)
}

/// Trade a one-time code and its verifier for a session, and keep the session.
async fn exchange_session(
	store: &Keystore,
	code: &str,
	verifier: &str,
) -> Result<SignInResult, String> {
	let res = post_json(
		&format!("{}/api/auth/desktop/exchange", api_base_url()),
		&serde_json::json!({ "code": code, "verifier": verifier }),
	)
	.await?;
	if !res.status().is_success() {
		return Err("That sign-in is no longer valid. Try again.".into());
	}

	let text = res
		.text()
		.await
		.map_err(|e| format!("Unexpected reply from Anthers: {e}"))?;
	let body: serde_json::Value =
		serde_json::from_str(&text).map_err(|e| format!("Unexpected reply from Anthers: {e}"))?;
	let token = body
		.get("token")
		.and_then(|t| t.as_str())
		.ok_or("Anthers did not return a session.")?;

	store.write(token)?;

	Ok(SignInResult {
		token: token.to_string(),
		persistence: Persistence::Keychain,
	})
}

/// Redeem a callback code for the session, and keep the session.
#[tauri::command]
pub async fn complete_sign_in(
	app: AppHandle,
	store: State<'_, Keystore>,
	code: String,
) -> Result<SignInResult, String> {
	let pending =
		read_pending(&app).ok_or("This sign-in didn't start here. Try signing in again.")?;
	let result = exchange_session(&store, &code, &pending.verifier).await?;
	forget_pending(&app);
	Ok(result)
}

/// Wait for the enrollment to be approved, then redeem it.
///
/// 🚨 **The server is asked, rather than waited on.** The alternative — a one-time code handed
/// back over a verified `anthers.org` app link — needs a browser that hands off and a process
/// Android has not reclaimed, and one failure among those leaves the person signed in on the
/// web and signed out in the app with nothing to retry. Polling the challenge this app itself
/// registered makes the browser only the place a password is typed.
#[tauri::command]
pub async fn await_sign_in(
	app: AppHandle,
	store: State<'_, Keystore>,
) -> Result<SignInResult, String> {
	let pending =
		read_pending(&app).ok_or("This sign-in didn't start here. Try signing in again.")?;
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
	loop {
		if std::time::Instant::now() >= deadline {
			return Err("That sign-in took too long. Start it again from this screen.".into());
		}
		match get_json(&format!(
			"{}/api/auth/desktop/poll/{}",
			api_base_url(),
			pending.challenge
		))
		.await
		{
			// 202: the browser leg has not been approved yet. Anything else is an answer.
			Ok(res) if res.status().as_u16() == 202 => {}
			Ok(res) if res.status().is_success() => {
				let text = res
					.text()
					.await
					.map_err(|e| format!("Unexpected reply from Anthers: {e}"))?;
				let body: serde_json::Value = serde_json::from_str(&text)
					.map_err(|e| format!("Unexpected reply from Anthers: {e}"))?;
				if let Some(code) = body.get("code").and_then(|code| code.as_str()) {
					let result = exchange_session(&store, code, &pending.verifier).await?;
					forget_pending(&app);
					return Ok(result);
				}
			}
			// A challenge the server no longer holds: it expired, or another path redeemed it.
			Ok(_) => return Err("That sign-in expired. Start it again from this screen.".into()),
			// A network blip is not an answer; keep asking until the deadline.
			Err(_) => {}
		}
		tokio::time::sleep(std::time::Duration::from_secs(2)).await;
	}
}

/// The stored session token, for the page to ask for when it boots.
///
/// 🚨 **A pull, not a push.** The shell used to evaluate the token into the page when its
/// keystore read finished, which lands in whatever document exists at that moment — often the
/// blank one that the app's real page then replaces, taking the token with it. Asking on boot
/// is the page's own business and cannot be mistimed.
#[tauri::command]
pub async fn session_token(store: State<'_, Keystore>) -> Result<Option<String>, String> {
	Ok(store.read())
}

/// Whether a sign-in is still waiting to be redeemed, so a launch can resume waiting for it.
#[tauri::command]
pub fn has_pending_sign_in(app: AppHandle) -> bool {
	read_pending(&app).is_some()
}

/// A code that arrived before the webview could listen, taken once.
#[tauri::command]
pub fn take_pending_code(code: State<'_, PendingCode>) -> Option<String> {
	code.0.lock().ok().and_then(|mut slot| slot.take())
}

/// Drop the stored session. Safe even when the server-side revocation failed.
#[tauri::command]
pub async fn sign_out(store: State<'_, Keystore>) -> Result<(), String> {
	store.clear()
}

/// The one-time code out of an app link, when it is the one this app is waiting for.
///
/// Only `https://anthers.org/auth/callback?code=…` counts. The host is checked rather than
/// assumed because the intent filter is not the only way a URL reaches `on_open_url`, and a
/// code from somewhere else is not one this process minted a verifier for.
pub fn callback_code(url: &tauri::Url) -> Option<String> {
	if url.scheme() != "https" || url.host_str() != Some("anthers.org") {
		return None;
	}
	if url.path() != "/auth/callback" {
		return None;
	}
	url.query_pairs()
		.find(|(key, _)| key == "code")
		.map(|(_, value)| value.into_owned())
		.filter(|code| !code.is_empty())
}

#[cfg(test)]
mod tests {
	use super::callback_code;
	use tauri::Url;

	fn code(url: &str) -> Option<String> {
		callback_code(&Url::parse(url).unwrap())
	}

	#[test]
	fn reads_the_code_the_authorize_page_hands_back() {
		assert_eq!(
			code("https://anthers.org/auth/callback?code=abc123").as_deref(),
			Some("abc123")
		);
		assert_eq!(
			code("https://anthers.org/auth/callback?state=x&code=abc123").as_deref(),
			Some("abc123")
		);
	}

	#[test]
	fn refuses_anything_that_is_not_this_callback() {
		// A different host, a different scheme, a different path, a missing code: none of them
		// is a code this process minted a verifier for, and redeeming one would be trusting a
		// link somebody else composed.
		assert_eq!(
			code("https://anthers.example/auth/callback?code=abc123"),
			None
		);
		assert_eq!(code("https://evil.example/auth/callback?code=abc123"), None);
		assert_eq!(code("http://anthers.org/auth/callback?code=abc123"), None);
		assert_eq!(code("https://anthers.org/works/abc?code=abc123"), None);
		assert_eq!(code("https://anthers.org/auth/callback"), None);
		assert_eq!(code("https://anthers.org/auth/callback?code="), None);
	}
}

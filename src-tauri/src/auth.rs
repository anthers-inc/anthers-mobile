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

use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::keystore::Keystore;

/// The PKCE verifier for the enrolment in flight. One sign-in at a time; starting another
/// replaces it.
#[derive(Default)]
pub struct PendingVerifier(Mutex<Option<String>>);

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

/// Mint a PKCE challenge, register it with the server, and open the authorize page in the
/// system browser. Returns the opened URL so the UI can offer it as copyable text — a button
/// that silently does nothing on a device with no browser handler is a dead end.
#[tauri::command]
pub async fn begin_sign_in(
	pending: State<'_, PendingVerifier>,
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

	// Only keep the verifier once the server has accepted its challenge, so a failed start
	// cannot leave a stale verifier a later callback would try to redeem.
	if let Ok(mut slot) = pending.0.lock() {
		*slot = Some(verifier);
	}

	// `redirect=mobile` is what tells the page to answer on the app link rather than on the
	// desktop's custom scheme. See `DesktopAuthorizePage` in the platform repository.
	let url = format!(
		"{}/desktop/authorize?challenge={challenge}&redirect=mobile",
		web_base_url()
	);
	tauri_plugin_opener::open_url(&url, None::<&str>)
		.map_err(|e| format!("Could not open your browser: {e}"))?;
	Ok(url)
}

/// Redeem a callback code for the session, and keep the session.
#[tauri::command]
pub async fn complete_sign_in(
	pending: State<'_, PendingVerifier>,
	store: State<'_, Keystore>,
	code: String,
) -> Result<SignInResult, String> {
	let verifier = pending
		.0
		.lock()
		.ok()
		.and_then(|mut slot| slot.take())
		.ok_or("This sign-in didn't start here. Try signing in again.")?;

	let res = post_json(
		&format!("{}/api/auth/desktop/exchange", api_base_url()),
		&serde_json::json!({ "code": code, "verifier": verifier }),
	)
	.await?;
	if !res.status().is_success() {
		return Err("That sign-in link is no longer valid. Try again.".into());
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

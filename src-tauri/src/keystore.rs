// SPDX-License-Identifier: Apache-2.0

//! The session token at rest.
//!
//! On Android the token is held by `KeystorePlugin.kt` in the app's own Android source: an
//! AES-256 key generated in the `AndroidKeyStore`, never exportable, encrypting the session
//! into the app's private files directory. The key material cannot leave the device, which is
//! the property the desktop gets from the OS keychain and which `keyring` — the crate the
//! desktop uses — has no Android backend for.
//!
//! ⚠️ **Written here rather than taken from a crate on purpose.** The community Tauri keystore
//! plugins require Android 9 / API 28, and this app's floor is API 24; a session credential is
//! also worth having the whole implementation of, in a file short enough to read.
//!
//! 🚨 **Every method here blocks on the Android main thread.** `run_mobile_plugin` posts the
//! command to the looper and waits on a channel, so calling one of these *from* the main
//! thread deadlocks. Commands run on a worker; the startup read runs on its own thread.

/// The plugin handle the Kotlin side is reached through.
#[cfg(target_os = "android")]
pub struct Keystore(pub tauri::plugin::PluginHandle<tauri::Wry>);

/// Nothing to hold where there is no Android keystore to hold it.
#[cfg(not(target_os = "android"))]
pub struct Keystore;

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
struct WritePayload<'a> {
	value: &'a str,
}

#[cfg(target_os = "android")]
#[derive(serde::Deserialize)]
struct ReadResult {
	value: Option<String>,
}

impl Keystore {
	/// The stored session, or `None` when there is none or it cannot be read.
	///
	/// A keystore that refuses to answer is treated as "not signed in": the person signs in
	/// again, which is an annoyance, where the alternative — failing the launch — is not.
	pub fn read(&self) -> Option<String> {
		#[cfg(target_os = "android")]
		{
			let result: ReadResult = self
				.0
				.run_mobile_plugin("read", serde_json::json!({}))
				.ok()?;
			result.value.filter(|token| !token.is_empty())
		}
		#[cfg(not(target_os = "android"))]
		{
			None
		}
	}

	/// Persist the session, replacing whatever was there.
	pub fn write(&self, token: &str) -> Result<(), String> {
		#[cfg(target_os = "android")]
		{
			self.0
				.run_mobile_plugin::<serde_json::Value>("write", WritePayload { value: token })
				.map(|_| ())
				.map_err(|e| format!("Could not store the session on this device: {e}"))
		}
		#[cfg(not(target_os = "android"))]
		{
			let _ = token;
			Err("This build has no keystore to store a session in.".into())
		}
	}

	/// Encrypt a scratch value and read it back, to prove the keystore works on this device.
	///
	/// Debug builds only — see the call in `lib.rs`. It never touches the session itself.
	#[cfg(target_os = "android")]
	pub fn selftest(&self) -> bool {
		#[derive(serde::Deserialize)]
		struct Result {
			ok: bool,
		}
		self.0
			.run_mobile_plugin::<Result>("selftest", serde_json::json!({}))
			.map(|r| r.ok)
			.unwrap_or(false)
	}

	/// Forget the stored session. Safe when there is none.
	pub fn clear(&self) -> Result<(), String> {
		#[cfg(target_os = "android")]
		{
			self.0
				.run_mobile_plugin::<serde_json::Value>("clear", serde_json::json!({}))
				.map(|_| ())
				.map_err(|e| format!("Could not clear the stored session: {e}"))
		}
		#[cfg(not(target_os = "android"))]
		{
			Ok(())
		}
	}
}

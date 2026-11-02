// SPDX-License-Identifier: Apache-2.0
package org.anthers.mobile

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The session token at rest, encrypted with a key the Android Keystore holds and nothing can
 * export.
 *
 * The key is generated on first write and lives in `AndroidKeyStore` under a fixed alias, in
 * this app's own namespace; it never leaves the device, and neither it nor the ciphertext is
 * readable by another app. The ciphertext goes in the app's private files directory, which
 * matters only as a place to keep bytes — the security is the key, not the path.
 *
 * ⚠️ Written here rather than taken from a crate because the community Tauri keystore plugins
 * require Android 9 (API 28) and this app supports API 24, and because a session credential is
 * worth having the whole implementation of in one readable file.
 *
 * ⚠️ Every method is called from Rust on a worker thread, never from JavaScript: no capability
 * permission is declared for this plugin on purpose, so the webview cannot read the token out
 * of storage even if a page is compromised. It only ever sees the token the shell injected.
 */
@InvokeArg
internal class WriteArgs {
    lateinit var value: String
}

@TauriPlugin
class KeystorePlugin(private val activity: Activity) : Plugin(activity) {
    private companion object {
        const val ALIAS = "org.anthers.mobile.session"
        const val FILE = "session.bin"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val TAG_BITS = 128
    }

    private val file: File get() = File(activity.filesDir, FILE)

    /** The stored session, or null when there is none — or when it cannot be decrypted. */
    @Command
    fun read(invoke: Invoke) {
        val result = JSObject()
        result.put("value", readToken())
        invoke.resolve(result)
    }

    @Command
    fun write(invoke: Invoke) {
        val args = invoke.parseArgs(WriteArgs::class.java)
        writeToken(args.value)
        invoke.resolve(JSObject())
    }

    @Command
    fun clear(invoke: Invoke) {
        // Best-effort: the file going away is what "signed out" means here, and a failure to
        // delete it is not a reason to leave the person signed in.
        file.delete()
        invoke.resolve(JSObject())
    }

    /**
     * Encrypt, write, read, decrypt, compare — on a scratch file, never the session.
     *
     * The keystore is the one part of sign-in that no test on a build machine can reach: it
     * exists only on a device. This is what a debug build calls at startup so the round trip
     * is observable in logcat instead of being assumed.
     */
    @Command
    fun selftest(invoke: Invoke) {
        val probe = "probe-" + System.nanoTime()
        val scratch = File(activity.filesDir, "selftest.bin")
        val ok =
            try {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(Cipher.ENCRYPT_MODE, key())
                val body = cipher.doFinal(probe.toByteArray(Charsets.UTF_8))
                scratch.writeBytes(cipher.iv + body)

                val bytes = scratch.readBytes()
                val read = Cipher.getInstance(TRANSFORMATION)
                read.init(
                    Cipher.DECRYPT_MODE,
                    key(),
                    GCMParameterSpec(TAG_BITS, bytes.copyOfRange(0, IV_BYTES)),
                )
                String(read.doFinal(bytes.copyOfRange(IV_BYTES, bytes.size)), Charsets.UTF_8) == probe
            } catch (e: Exception) {
                false
            } finally {
                scratch.delete()
            }
        val result = JSObject()
        result.put("ok", ok)
        invoke.resolve(result)
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry
        if (existing != null) return existing.secretKey

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private fun writeToken(token: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val body = cipher.doFinal(token.toByteArray(Charsets.UTF_8))
        // The IV is generated per encryption and has to travel with the ciphertext, which is
        // why it is the first twelve bytes rather than a constant.
        file.writeBytes(cipher.iv + body)
    }

    private fun readToken(): String? =
        try {
            val bytes = file.readBytes()
            if (bytes.size <= IV_BYTES) {
                null
            } else {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(
                    Cipher.DECRYPT_MODE,
                    key(),
                    GCMParameterSpec(TAG_BITS, bytes.copyOfRange(0, IV_BYTES)),
                )
                String(cipher.doFinal(bytes.copyOfRange(IV_BYTES, bytes.size)), Charsets.UTF_8)
            }
        } catch (e: Exception) {
            // A key that was invalidated (a device credential change, a restored backup) or a
            // truncated file: the person signs in again, which is an annoyance rather than a
            // failed launch.
            null
        }
}

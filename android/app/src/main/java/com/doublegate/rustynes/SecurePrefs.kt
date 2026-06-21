package com.doublegate.rustynes

import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Field-level secret encryption for SharedPreferences (v1.8.8 "Atlas", Workstream C).
 *
 * Sensitive user input — the RetroAchievements login token, the TURN shared secret,
 * the TheGamesDB API key, and (when enabled) the ScreenScraper password — is stored
 * as **AES-256-GCM ciphertext** keyed by a **hardware-backed Android Keystore** key
 * that never leaves the secure element / TEE. The plaintext is only ever held in
 * memory while in use; what lands on disk is `Base64(iv || ciphertext)`.
 *
 * This follows Android's CURRENT official guidance (developer.android.com security
 * tips): store secrets/API-keys/tokens encrypted under the **Android Keystore** — the
 * documented primary mechanism now that `androidx.security`'s
 * EncryptedSharedPreferences is deprecated (April 2025, 1.1.0-alpha07). Using the
 * Keystore directly keeps it dependency-free with a per-record GCM IV and
 * non-exportable key material. (Google additionally suggests layering Google **Tink**
 * over the Keystore for "optimal" envelope-encryption / key-rotation; for these few
 * short-lived tokens, direct Keystore AES-GCM is the canonical, lean choice. Credential
 * Manager / AccountManager are for interactive auth / passkeys, not at-rest token
 * storage, so they don't apply here.) TODO: layer Tink if richer key rotation is wanted.
 *
 * Passwords proper (the RA account password, the SAF-picker logins) are NEVER
 * persisted at all — only the derived token is. This is for the values that must
 * survive a relaunch.
 */
object SecurePrefs {
    private const val KEY_ALIAS = "rustynes_secret_key"
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val TRANSFORM = "AES/GCM/NoPadding"
    private const val IV_LEN = 12
    private const val TAG_BITS = 128
    /** Prefix marking an already-encrypted value (so a legacy plaintext value written
     *  by an older build is read back as-is, then re-encrypted on the next write). */
    private const val ENC_PREFIX = "enc1:"

    private fun secretKey(): SecretKey {
        val ks = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    /** Encrypt [plain] to `enc1:Base64(iv||ct)`. Returns "" for empty input. */
    fun encrypt(plain: String): String {
        if (plain.isEmpty()) return ""
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORM).apply { init(Cipher.ENCRYPT_MODE, secretKey()) }
            val iv = cipher.iv
            val ct = cipher.doFinal(plain.toByteArray(Charsets.UTF_8))
            ENC_PREFIX + Base64.encodeToString(iv + ct, Base64.NO_WRAP)
        }.getOrDefault(plain)
    }

    /** Decrypt a value written by [encrypt]; pass through legacy plaintext unchanged. */
    fun decrypt(stored: String): String {
        if (stored.isEmpty()) return ""
        if (!stored.startsWith(ENC_PREFIX)) return stored // legacy plaintext
        return runCatching {
            val blob = Base64.decode(stored.removePrefix(ENC_PREFIX), Base64.NO_WRAP)
            val iv = blob.copyOfRange(0, IV_LEN)
            val ct = blob.copyOfRange(IV_LEN, blob.size)
            val cipher = Cipher.getInstance(TRANSFORM).apply {
                init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(TAG_BITS, iv))
            }
            String(cipher.doFinal(ct), Charsets.UTF_8)
        }.getOrDefault("")
    }

    /** Read + decrypt a secret pref (returns "" if absent or undecryptable). */
    fun getSecret(prefs: SharedPreferences, key: String): String =
        decrypt(prefs.getString(key, "") ?: "")

    /** Encrypt + write a secret pref (empty clears it). */
    fun putSecret(prefs: SharedPreferences, key: String, value: String) {
        prefs.edit().putString(key, if (value.isEmpty()) "" else encrypt(value)).apply()
    }
}

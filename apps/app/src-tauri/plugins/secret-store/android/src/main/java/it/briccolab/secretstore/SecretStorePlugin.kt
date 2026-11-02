package it.briccolab.secretstore

import android.app.Activity
import android.content.Context
import android.os.Build
import android.view.WindowManager
import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class NameArgs {
    lateinit var name: String
}

@InvokeArg
class PutArgs {
    lateinit var name: String
    /** Base64 of the secret bytes. */
    lateinit var value: String
}

/**
 * Device secrets (PROGETTO.md §8.1): each value is encrypted with an AES-GCM
 * key that never leaves the Android Keystore, and stored in the app's
 * private preferences. Uninstalling the app removes both.
 */
object SecretVault {
    private const val KEY_ALIAS = "app.secret-store"
    private const val PREFS = "secret-store"

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    fun get(context: Context, name: String): ByteArray? {
        val stored = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(name, null) ?: return null
        val bytes = Base64.decode(stored, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, 12))
        return cipher.doFinal(bytes, 12, bytes.size - 12)
    }

    fun put(context: Context, name: String, value: ByteArray) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(value)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString(name, Base64.encodeToString(sealed, Base64.NO_WRAP))
            .commit()
    }

    fun delete(context: Context, name: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().remove(name).commit()
    }
}

@TauriPlugin
class SecretStorePlugin(private val activity: Activity) : Plugin(activity) {
    /** Hides the content in the recent-apps screen (checklist #11). */
    override fun load(webView: WebView) {
        super.load(webView)
        // Periodic sync while the app is closed (§11.4).
        SyncWorker.schedule(activity.applicationContext)
        activity.runOnUiThread {
            if (Build.VERSION.SDK_INT >= 33) {
                activity.setRecentsScreenshotEnabled(false)
            } else {
                activity.window.setFlags(
                    WindowManager.LayoutParams.FLAG_SECURE,
                    WindowManager.LayoutParams.FLAG_SECURE
                )
            }
        }
    }

    /** Leaving the app: send what is pending right away (§11.4). */
    override fun onStop() {
        SyncWorker.runSoon(activity.applicationContext)
    }

    /** Status and navigation bar sizes in CSS pixels, for the page margins. */
    @Command
    fun insets(invoke: Invoke) {
        activity.runOnUiThread {
            val ret = JSObject()
            val density = activity.resources.displayMetrics.density
            ViewCompat.getRootWindowInsets(activity.window.decorView)?.let {
                val bars = it.getInsets(
                    WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
                )
                ret.put("top", bars.top / density)
                ret.put("bottom", bars.bottom / density)
            }
            invoke.resolve(ret)
        }
    }

    @Command
    fun get(invoke: Invoke) {
        val args = invoke.parseArgs(NameArgs::class.java)
        try {
            val ret = JSObject()
            SecretVault.get(activity, args.name)?.let {
                ret.put("value", Base64.encodeToString(it, Base64.NO_WRAP))
            }
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("secret store: ${e.javaClass.simpleName}")
        }
    }

    @Command
    fun put(invoke: Invoke) {
        val args = invoke.parseArgs(PutArgs::class.java)
        try {
            SecretVault.put(activity, args.name, Base64.decode(args.value, Base64.NO_WRAP))
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("secret store: ${e.javaClass.simpleName}")
        }
    }

    @Command
    fun delete(invoke: Invoke) {
        val args = invoke.parseArgs(NameArgs::class.java)
        SecretVault.delete(activity, args.name)
        invoke.resolve()
    }
}

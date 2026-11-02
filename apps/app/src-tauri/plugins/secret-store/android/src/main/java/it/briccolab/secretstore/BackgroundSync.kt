package it.briccolab.secretstore

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/** Secrets for the Rust core when it runs without the app (JNI). */
class VaultBridge(private val context: Context) {
    fun get(name: String): ByteArray? = SecretVault.get(context, name)
    fun put(name: String, value: ByteArray) = SecretVault.put(context, name, value)
    fun delete(name: String) = SecretVault.delete(context, name)
}

/** Entry point of the Rust core for background runs (libapp_lib.so). */
object NativeSync {
    init {
        System.loadLibrary("app_lib")
    }

    @JvmStatic
    external fun runOnce(dataDir: String, vault: Any): Boolean
}

/** Short sync (max ~25 s) every 15 minutes or so, when there is network (§11.4). */
class SyncWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        val ok = try {
            NativeSync.runOnce(applicationContext.dataDir.absolutePath, VaultBridge(applicationContext))
        } catch (e: Throwable) {
            false
        }
        return if (ok) Result.success() else Result.retry()
    }

    companion object {
        private const val NAME = "orgapp-sync"

        /** One short run as soon as there is network (e.g. when leaving the app). */
        fun runSoon(context: Context) {
            val request = OneTimeWorkRequestBuilder<SyncWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context)
                .enqueueUniqueWork("$NAME-now", ExistingWorkPolicy.REPLACE, request)
        }

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }
    }
}

//! Android background sync (PROGETTO.md §11.4): a periodic WorkManager job
//! (Kotlin, `secret-store` plugin) calls [`run_once`] through JNI. If the app
//! is open the running core syncs; otherwise a core is opened just for this
//! run, with the secrets read through the Kotlin vault, and closed again.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use core_api::{Core, CoreOptions};
use core_crypto::{SecretStore, SecretStoreError};
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean};
use jni::{JNIEnv, JavaVM};
use zeroize::Zeroizing;

/// The core of the running app, if any.
pub static RUNNING: OnceLock<Arc<Core>> = OnceLock::new();

/// Longest background run (the system allows about 10 minutes; we stay short).
const MAX_RUN: Duration = Duration::from_secs(25);

/// Secrets through `it.briccolab.secretstore.VaultBridge` (Keystore).
struct JniSecretStore {
    vm: JavaVM,
    bridge: GlobalRef,
}

impl JniSecretStore {
    fn with_env<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<T>,
    ) -> Result<T, SecretStoreError> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|_| SecretStoreError::Unavailable)?;
        let r = f(&mut env);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
            return Err(SecretStoreError::Unavailable);
        }
        r.map_err(|_| SecretStoreError::Unavailable)
    }
}

impl SecretStore for JniSecretStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        self.with_env(|env| {
            let n = env.new_string(name)?;
            let v = env
                .call_method(
                    self.bridge.as_obj(),
                    "get",
                    "(Ljava/lang/String;)[B",
                    &[JValue::Object(&n)],
                )?
                .l()?;
            if v.is_null() {
                return Ok(None);
            }
            let bytes = env.convert_byte_array(JByteArray::from(v))?;
            Ok(Some(Zeroizing::new(bytes)))
        })
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        self.with_env(|env| {
            let n = env.new_string(name)?;
            let v = env.byte_array_from_slice(value)?;
            env.call_method(
                self.bridge.as_obj(),
                "put",
                "(Ljava/lang/String;[B)V",
                &[JValue::Object(&n), JValue::Object(&v)],
            )?;
            Ok(())
        })
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.with_env(|env| {
            let n = env.new_string(name)?;
            env.call_method(
                self.bridge.as_obj(),
                "delete",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&n)],
            )?;
            Ok(())
        })
    }
}

async fn sync_with(core: &Arc<Core>) -> bool {
    let run = async {
        let _ = core.start_network().await;
        core.sync_round().await.is_ok()
    };
    tokio::time::timeout(MAX_RUN, run).await.unwrap_or(false)
}

fn run_once(env: &mut JNIEnv, data_dir: &JString, bridge: &JObject) -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    if let Some(core) = RUNNING.get() {
        return runtime.block_on(sync_with(core));
    }
    let Ok(dir) = env.get_string(data_dir).map(String::from) else {
        return false;
    };
    let (Ok(vm), Ok(bridge)) = (env.get_java_vm(), env.new_global_ref(bridge)) else {
        return false;
    };
    let secrets = Arc::new(JniSecretStore { vm, bridge });
    let opened = Core::open(CoreOptions {
        data_dir: dir.into(),
        bundle: crate::BUNDLE.to_vec(),
        secrets,
        network: Some(crate::network()),
        clock: None,
        pin_params: None,
        server: crate::server(),
        security: None,
    });
    let Ok(core) = opened else { return false };
    if !core.has_org() {
        return false;
    }
    runtime.block_on(sync_with(&core))
}

/// `it.briccolab.secretstore.NativeSync.runOnce(dataDir, vault): Boolean`
// JNI finds the function by this exact symbol name.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_it_briccolab_secretstore_NativeSync_runOnce(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    bridge: JObject,
) -> jboolean {
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_once(&mut env, &data_dir, &bridge)
    }))
    .unwrap_or(false);
    if ok { JNI_TRUE } else { JNI_FALSE }
}

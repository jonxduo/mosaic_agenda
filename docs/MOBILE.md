# App mobile (Android e iOS)

Stesso nucleo e stessa interfaccia del desktop, dentro Tauri 2. Il progetto nativo
(`apps/app/src-tauri/gen/android`, `gen/apple`) è **generato per pacchetto** da
`cargo xtask build` (l'identificativo dell'app viene dal pacchetto) e non è nel repository.
Le parti native nostre stanno nei plugin in `apps/app/src-tauri/plugins/`.

## Strumenti

| Cosa | Come |
| --- | --- |
| Rust per mobile | `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios` |
| Android SDK e NDK | `brew install --cask android-commandlinetools`, poi `sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;29.0.14206865"` (Java 17+) |
| Emulatore (facoltativo) | `sdkmanager "emulator" "system-images;android-36;google_apis;arm64-v8a"` e `avdmanager create avd -n orgapp-test -k "system-images;android-36;google_apis;arm64-v8a" -d pixel_7` |
| iOS | Xcode completo (App Store), licenza accettata (`sudo xcodebuild -license accept`), `xcode-select -s /Applications/Xcode.app`, runtime del simulatore (`xcodebuild -downloadPlatform iOS`), `brew install xcodegen cocoapods` |

`cargo xtask` trova da solo l'SDK Android in `/opt/homebrew/share/android-commandlinetools`
(oppure `ANDROID_HOME` / `NDK_HOME`).

## Build

```bash
# APK di debug per telefoni arm64 ed emulatore
cargo xtask build --package congregazione --target android [--server <dominio>]
# installazione sul telefono collegato via USB (debug USB attivo) o sull'emulatore
adb install -r apps/app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk

# iOS: simulatore Apple Silicon (debug) — serve Xcode
cargo xtask build --package congregazione --target ios [--server <dominio>]
# installazione e avvio sul simulatore acceso
xcrun simctl install booted apps/app/src-tauri/gen/apple/build/arm64-sim/Congregazione.app
xcrun simctl launch booted it.briccolab.congregazione
```

Le voci aggiunte al plist iOS (descrizioni d'uso di fotocamera e Face ID) stanno in
`apps/app/src-tauri/Info.ios.plist`; i framework di sistema in `tauri.conf.json`
(`bundle.iOS.frameworks`). Cambiando `tauri.conf.json` il progetto nativo si rigenera.

Per lo store: `--release` produce l'AAB (Android) o l'archivio per App Store Connect (iOS);
servono le chiavi di firma (sotto).

## Firma

- **Android**: un keystore di caricamento (`keytool -genkey -v -keystore upload.jks -alias upload
  -keyalg RSA -keysize 2048 -validity 10000`), tenuto fuori dal repository; Google Play gestisce
  la chiave di firma finale (Play App Signing).
- **iOS**: account Apple Developer (99 €/anno), team id nel progetto Xcode generato
  (`APPLE_DEVELOPMENT_TEAM` per Tauri), profilo di provisioning per l'identificativo
  `it.briccolab.congregazione`.

## Cosa fanno i plugin

| Plugin | Cosa | Dove |
| --- | --- | --- |
| `secret-store` (nostro) | Chiavi del dispositivo e del database nel Keystore Android (AES-GCM, chiave non esportabile) o nel Keychain iOS (accessibile dopo il primo sblocco, mai sincronizzato); contenuto nascosto nel selettore delle app | `plugins/secret-store` |
| `biometric` (ufficiale) | Impronta o volto prima di rilasciare la chiave del database | Impostazioni → "Sblocco con impronta o volto" |
| `barcode-scanner` (ufficiale) | QR dell'invito per abbinare il telefono | Primo avvio → "Inquadra il codice QR" |
| `notification` (ufficiale) | Notifiche locali | come su desktop |

## Push (da completare con le credenziali)

- **Android**: serve un progetto Firebase con un'app Android `it.briccolab.congregazione`: il file
  `google-services.json` (per l'app) e la chiave del service account (per il server, vedi
  docs/DEPLOY.md §5).
- **iOS**: chiave APNs `.p8` dell'account Apple Developer (server) e la capability "Push
  Notifications" nel progetto.

L'app registra il token sul servizio push del proprio server (`register_push`) e il servizio la
sveglia quando qualcuno le manda qualcosa e lei non ha confermato la ricezione entro 10 secondi.

## Prove

La checklist manuale è in [MOBILE_CHECKLIST.md](MOBILE_CHECKLIST.md).

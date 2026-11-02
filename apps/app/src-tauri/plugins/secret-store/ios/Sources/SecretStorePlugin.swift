import Foundation
import Security
import SwiftRs
import Tauri
import UIKit
import WebKit

class NameArgs: Decodable {
  let name: String
}

class PutArgs: Decodable {
  let name: String
  /// Base64 of the secret bytes.
  let value: String
}

/// Device secrets (PROGETTO.md §8.1) in the iOS Keychain: readable after the
/// first unlock of the phone (so background sync works), never synced to
/// other devices or included in backups.
class SecretStorePlugin: Plugin {
  let service = Bundle.main.bundleIdentifier ?? "app"
  private var cover: UIView?

  /// Hides the content in the app switcher (checklist #11).
  @objc public override func load(webview: WKWebView) {
    super.load(webview: webview)
    let center = NotificationCenter.default
    center.addObserver(
      self, selector: #selector(hide), name: UIApplication.willResignActiveNotification, object: nil)
    center.addObserver(
      self, selector: #selector(show), name: UIApplication.didBecomeActiveNotification, object: nil)
  }

  @objc private func hide() {
    guard cover == nil,
      let window = UIApplication.shared.connectedScenes
        .compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first
    else { return }
    let blur = UIVisualEffectView(effect: UIBlurEffect(style: .systemMaterial))
    blur.frame = window.bounds
    blur.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    window.addSubview(blur)
    cover = blur
  }

  @objc private func show() {
    cover?.removeFromSuperview()
    cover = nil
  }

  /// iOS: the web view already exposes the safe areas to CSS (env()).
  @objc public func insets(_ invoke: Invoke) throws {
    invoke.resolve([:])
  }

  private func query(_ name: String) -> [String: Any] {
    return [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: name,
    ]
  }

  @objc public func get(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(NameArgs.self)
    var q = query(args.name)
    q[kSecReturnData as String] = true
    q[kSecMatchLimit as String] = kSecMatchLimitOne
    var item: CFTypeRef?
    let status = SecItemCopyMatching(q as CFDictionary, &item)
    if status == errSecItemNotFound {
      invoke.resolve([:])
    } else if status == errSecSuccess, let data = item as? Data {
      invoke.resolve(["value": data.base64EncodedString()])
    } else {
      invoke.reject("secret store: keychain error \(status)")
    }
  }

  @objc public func put(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PutArgs.self)
    guard let data = Data(base64Encoded: args.value) else {
      invoke.reject("secret store: bad value")
      return
    }
    SecItemDelete(query(args.name) as CFDictionary)
    var q = query(args.name)
    q[kSecValueData as String] = data
    q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let status = SecItemAdd(q as CFDictionary, nil)
    if status == errSecSuccess {
      invoke.resolve()
    } else {
      invoke.reject("secret store: keychain error \(status)")
    }
  }

  @objc public func delete(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(NameArgs.self)
    SecItemDelete(query(args.name) as CFDictionary)
    invoke.resolve()
  }
}

@_cdecl("init_plugin_secret_store")
public func initPlugin() -> Plugin {
  return SecretStorePlugin()
}

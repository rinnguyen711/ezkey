// ezkey for macOS: a menu bar app that rewrites keystrokes with a CGEventTap.
// Ctrl+Shift (or the menu) toggles Vietnamese / English.
// Needs System Settings → Privacy & Security → Accessibility permission.

import Carbon.HIToolbox
import Cocoa
import ServiceManagement

/// Marks events we post so the tap lets them through untouched.
let injectedTag: Int64 = 0x455A_4B59

/// Settings are kept in UserDefaults under these keys.
enum Setting {
    static let enabled = "Enabled"
    static let modern = "ModernTone"
    static let vni = "Vni"
    static let perApp = "PerApp"
    /// Bundle ID → Vietnamese on, the mode last chosen in each app.
    static let apps = "Apps"
}

final class Ime {
    let engine = ezkey_engine_new()!
    let defaults = UserDefaults.standard
    var enabled: Bool
    var modern = false
    var vni = false
    /// Remember Vietnamese / English per app.
    var perApp: Bool
    /// Bundle ID of the app the user is typing in.
    var app = NSWorkspace.shared.frontmostApplication?.bundleIdentifier
    /// Ctrl and Shift are both held with no other key pressed in between.
    var toggleArmed = false
    var tap: CFMachPort?
    var onToggle: () -> Void = {}

    init() {
        enabled = defaults.object(forKey: Setting.enabled) as? Bool ?? true
        perApp = defaults.object(forKey: Setting.perApp) as? Bool ?? true
        setModern(defaults.bool(forKey: Setting.modern))
        setVni(defaults.bool(forKey: Setting.vni))
    }

    func toggle() {
        enabled.toggle()
        defaults.set(enabled, forKey: Setting.enabled)
        ezkey_engine_reset(engine)
        rememberForApp()
        onToggle()
    }

    func rememberForApp() {
        guard perApp, let app else { return }
        var apps = defaults.dictionary(forKey: Setting.apps) ?? [:]
        apps[app] = enabled
        defaults.set(apps, forKey: Setting.apps)
    }

    func setPerApp(_ on: Bool) {
        perApp = on
        defaults.set(on, forKey: Setting.perApp)
        // Start from the current mode for the app the user came from.
        rememberForApp()
    }

    /// Another app came to the front: switch to the mode last chosen there.
    func activated(_ bundleID: String?) {
        guard let bundleID, bundleID != Bundle.main.bundleIdentifier, bundleID != app else { return }
        app = bundleID
        ezkey_engine_reset(engine)
        guard perApp, let remembered = defaults.dictionary(forKey: Setting.apps)?[bundleID] as? Bool,
              remembered != enabled else { return }
        enabled = remembered
        defaults.set(enabled, forKey: Setting.enabled)
        onToggle()
    }

    func setModern(_ on: Bool) {
        modern = on
        defaults.set(on, forKey: Setting.modern)
        ezkey_engine_set_modern_style(engine, on)
        ezkey_engine_reset(engine)
    }

    func setVni(_ on: Bool) {
        vni = on
        defaults.set(on, forKey: Setting.vni)
        ezkey_engine_set_vni(engine, on)
    }
}

let ime = Ime()

/// Debug log of every key event in and out, enabled by starting ezkey with
/// `open --env EZKEY_LOG=/tmp/ezkey.log macos/build/ezkey.app`.
let logFile: FileHandle? = ProcessInfo.processInfo.environment["EZKEY_LOG"].flatMap { path in
    FileManager.default.createFile(atPath: path, contents: nil)
    return FileHandle(forWritingAtPath: path)
}

func log(_ line: String) {
    logFile?.write((line + "\n").data(using: .utf8)!)
}

func describe(_ e: CGEvent) -> String {
    var length = 0
    var chars = [UniChar](repeating: 0, count: 8)
    e.keyboardGetUnicodeString(maxStringLength: 8, actualStringLength: &length, unicodeString: &chars)
    let text = String(utf16CodeUnits: chars, count: length).debugDescription
    let field = { (f: CGEventField) in e.getIntegerValueField(f) }
    return String(format: "t=%.3fms type=%d key=%lld repeat=%lld flags=%llx kbd=%lld src=%lld pid=%lld chars=%@",
                  Double(e.timestamp) / 1e6, e.type.rawValue, field(.keyboardEventKeycode),
                  field(.keyboardEventAutorepeat), e.flags.rawValue, field(.keyboardEventKeyboardType),
                  field(.eventSourceStateID), field(.eventSourceUnixProcessID), text)
}

func post(_ event: CGEvent, _ proxy: CGEventTapProxy, flags: CGEventFlags = []) {
    event.setIntegerValueField(.eventSourceUserData, value: injectedTag)
    event.flags = flags
    if logFile != nil { log("  out " + describe(event)) }
    event.tapPostEvent(proxy)
}

func sendKey(_ key: Int, flags: CGEventFlags = [], source: CGEventSource?, proxy: CGEventTapProxy) {
    for down in [true, false] {
        if let e = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(key), keyDown: down) {
            post(e, proxy, flags: flags)
        }
    }
}

/// Browsers whose address bar inline-autocompletes: after "a" the bar holds
/// "a[mazon.com]" with the suggestion selected, so a plain Backspace deletes
/// the suggestion instead of the "a" and "aa" ends up as "aâ".
/// Bundle ID prefixes, so beta/canary builds match too.
let chromiumBrowsers = [
    "com.microsoft.edgemac",
    "com.google.Chrome",
    "com.brave.Browser",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "company.thebrowser.Browser",
]
let otherBrowsers = ["org.mozilla.firefox", "com.apple.Safari"]

func frontmostMatches(_ ids: [String]) -> Bool {
    guard let id = NSWorkspace.shared.frontmostApplication?.bundleIdentifier else { return false }
    return ids.contains { id.hasPrefix($0) }
}

/// Typed first in Safari/Firefox: it replaces any selected autocomplete
/// suggestion and is then erased with one extra Backspace. Chromium on macOS
/// drops it, so the extra Backspace ate a real letter ("chào" → "cào").
let autocompleteBreaker = "\u{202F}"

/// Sends `backspaces` Backspaces, then `text` as presses of `keycode`.
func sendReplacement(backspaces: Int, text: String, keycode: CGKeyCode, proxy: CGEventTapProxy) {
    let source = CGEventSource(stateID: .privateState)
    var backspaces = backspaces
    if backspaces > 0 && frontmostMatches(chromiumBrowsers) {
        // Shift+Left selects the character before the caret along with any
        // suggestion, so the first Backspace deletes both.
        sendKey(kVK_LeftArrow, flags: .maskShift, source: source, proxy: proxy)
    } else if backspaces > 0 && frontmostMatches(otherBrowsers) {
        let units = Array(autocompleteBreaker.utf16)
        for down in [true, false] {
            if let e = CGEvent(keyboardEventSource: source, virtualKey: keycode, keyDown: down) {
                e.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
                post(e, proxy)
            }
        }
        backspaces += 1
    }
    for _ in 0..<backspaces {
        sendKey(kVK_Delete, source: source, proxy: proxy)
    }
    // One event per character: some apps only read the first one. They use
    // the typed key's code: the previous letter's key may still be held, and
    // Chromium drops a press of a key that is already down ("af" lost "à").
    for ch in text {
        let units = Array(String(ch).utf16)
        for down in [true, false] {
            if let e = CGEvent(keyboardEventSource: source, virtualKey: keycode, keyDown: down) {
                e.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
                post(e, proxy)
            }
        }
    }
}

func handle(proxy: CGEventTapProxy, type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
    let pass = Unmanaged.passUnretained(event)
    if logFile != nil && [.keyDown, .keyUp, .flagsChanged].contains(type) {
        log("in  " + describe(event) + " front=" + (NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?"))
    }
    switch type {
    case .tapDisabledByTimeout, .tapDisabledByUserInput:
        // Re-enabling after Accessibility was revoked freezes all input.
        if AXIsProcessTrusted(), let tap = ime.tap { CGEvent.tapEnable(tap: tap, enable: true) }
        return pass
    case .leftMouseDown, .rightMouseDown, .otherMouseDown:
        ezkey_engine_reset(ime.engine)
        return pass
    case .flagsChanged:
        let f = event.flags
        let both = f.contains(.maskControl) && f.contains(.maskShift)
            && !f.contains(.maskCommand) && !f.contains(.maskAlternate)
        if both {
            ime.toggleArmed = true
        } else if ime.toggleArmed {
            ime.toggleArmed = false
            ime.toggle()
        }
        return pass
    case .keyDown:
        break
    default:
        return pass
    }

    ime.toggleArmed = false
    if event.getIntegerValueField(.eventSourceUserData) == injectedTag || !ime.enabled {
        return pass
    }
    let flags = event.flags
    if flags.contains(.maskCommand) || flags.contains(.maskControl) || flags.contains(.maskAlternate) {
        ezkey_engine_reset(ime.engine)
        return pass
    }
    if event.getIntegerValueField(.keyboardEventKeycode) == Int64(kVK_Delete) {
        ezkey_engine_backspace(ime.engine)
        return pass
    }

    var length = 0
    var chars = [UniChar](repeating: 0, count: 4)
    event.keyboardGetUnicodeString(maxStringLength: 4, actualStringLength: &length, unicodeString: &chars)
    guard length == 1, let scalar = Unicode.Scalar(chars[0]) else {
        ezkey_engine_reset(ime.engine)
        return pass
    }

    // Letters, and digits for VNI; the engine ends the word on anything else.
    var backspaces: UInt32 = 0
    var out = [UInt32](repeating: 0, count: 64)
    let n = ezkey_engine_process(ime.engine, scalar.value, &backspaces, &out, UInt32(out.count))
    if n < 0 {
        return pass
    }
    var text = ""
    for code in out[0..<Int(n)] {
        if let s = Unicode.Scalar(code) { text.unicodeScalars.append(s) }
    }
    log("  replace backspaces=\(backspaces) text=\(text.debugDescription)")
    // The last character rides on the real key event, so the app sees a
    // genuine keystroke with its own key-up; the rest is sent before it.
    guard let last = text.last else {
        sendReplacement(backspaces: Int(backspaces), text: text, keycode: 0, proxy: proxy)
        return nil
    }
    let keycode = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
    sendReplacement(backspaces: Int(backspaces), text: String(text.dropLast()), keycode: keycode, proxy: proxy)
    let units = Array(String(last).utf16)
    event.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
    return pass
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var toggleItem: NSMenuItem!
    var telexItem: NSMenuItem!
    var vniItem: NSMenuItem!
    var modernItem: NSMenuItem!
    var perAppItem: NSMenuItem!
    var loginItem: NSMenuItem!
    var permissionTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        let menu = NSMenu()
        func add(_ title: String, _ action: Selector) -> NSMenuItem {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            menu.addItem(item)
            return item
        }
        toggleItem = add("Gõ tiếng Việt (Ctrl+Shift)", #selector(toggle))
        menu.addItem(.separator())
        telexItem = add("Telex", #selector(chooseTelex))
        vniItem = add("VNI", #selector(chooseVni))
        modernItem = add("Đặt dấu kiểu mới (hoà, thuý)", #selector(toggleModern))
        perAppItem = add("Nhớ chế độ V/E theo từng app", #selector(togglePerApp))
        menu.addItem(.separator())
        loginItem = add("Mở cùng máy", #selector(toggleLogin))
        if #unavailable(macOS 13) {
            loginItem.isHidden = true
        }
        menu.addItem(NSMenuItem(title: "Thoát", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        statusItem.menu = menu
        ime.onToggle = { [weak self] in self?.refresh() }
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            ime.activated(app?.bundleIdentifier)
        }
        refresh()

        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        if AXIsProcessTrustedWithOptions(options) {
            startTap()
        } else {
            // Wait until the user grants Accessibility access in System Settings.
            permissionTimer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] timer in
                if AXIsProcessTrusted() {
                    timer.invalidate()
                    self?.startTap()
                }
            }
        }
    }

    func startTap() {
        var types: [CGEventType] = [.keyDown, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown]
        if logFile != nil { types.append(.keyUp) }
        let mask = types.reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: mask,
            callback: { proxy, type, event, _ in handle(proxy: proxy, type: type, event: event) },
            userInfo: nil
        ) else {
            let alert = NSAlert()
            alert.messageText = "ezkey không tạo được event tap. Hãy cấp quyền Accessibility rồi mở lại."
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        ime.tap = tap
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)

        // A filtering tap left running without Accessibility permission blocks
        // every key and click system-wide, so quit as soon as it is revoked.
        Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
            if !AXIsProcessTrusted() {
                CGEvent.tapEnable(tap: tap, enable: false)
                CFMachPortInvalidate(tap)
                exit(0)
            }
        }
    }

    func refresh() {
        statusItem.button?.title = ime.enabled ? "V" : "E"
        toggleItem.state = ime.enabled ? .on : .off
        telexItem.state = ime.vni ? .off : .on
        vniItem.state = ime.vni ? .on : .off
        modernItem.state = ime.modern ? .on : .off
        perAppItem.state = ime.perApp ? .on : .off
        if #available(macOS 13, *) {
            loginItem.state = SMAppService.mainApp.status == .enabled ? .on : .off
        }
    }

    @objc func toggle() {
        ime.toggle()
    }

    @objc func chooseTelex() {
        ime.setVni(false)
        refresh()
    }

    @objc func chooseVni() {
        ime.setVni(true)
        refresh()
    }

    @objc func togglePerApp() {
        ime.setPerApp(!ime.perApp)
        refresh()
    }

    @objc func toggleModern() {
        ime.setModern(!ime.modern)
        refresh()
    }

    @objc func toggleLogin() {
        guard #available(macOS 13, *) else { return }
        let service = SMAppService.mainApp
        do {
            if service.status == .enabled {
                try service.unregister()
            } else {
                try service.register()
            }
        } catch {
            let alert = NSAlert()
            alert.messageText = "Không đổi được cài đặt mở cùng máy: \(error.localizedDescription)"
            alert.runModal()
        }
        refresh()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()

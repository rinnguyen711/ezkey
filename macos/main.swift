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
}

final class Ime {
    let engine = ezkey_engine_new()!
    let defaults = UserDefaults.standard
    var enabled: Bool
    var modern = false
    var vni = false
    /// Ctrl and Shift are both held with no other key pressed in between.
    var toggleArmed = false
    var tap: CFMachPort?
    var onToggle: () -> Void = {}

    init() {
        enabled = defaults.object(forKey: Setting.enabled) as? Bool ?? true
        setModern(defaults.bool(forKey: Setting.modern))
        setVni(defaults.bool(forKey: Setting.vni))
    }

    func toggle() {
        enabled.toggle()
        defaults.set(enabled, forKey: Setting.enabled)
        ezkey_engine_reset(engine)
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

func post(_ event: CGEvent, _ proxy: CGEventTapProxy) {
    event.setIntegerValueField(.eventSourceUserData, value: injectedTag)
    event.flags = []
    event.tapPostEvent(proxy)
}

func sendReplacement(backspaces: Int, text: String, proxy: CGEventTapProxy) {
    let source = CGEventSource(stateID: .privateState)
    for _ in 0..<backspaces {
        for down in [true, false] {
            if let e = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(kVK_Delete), keyDown: down) {
                post(e, proxy)
            }
        }
    }
    // One event per character: some apps only read the first one.
    for ch in text {
        let units = Array(String(ch).utf16)
        for down in [true, false] {
            if let e = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down) {
                e.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
                post(e, proxy)
            }
        }
    }
}

func handle(proxy: CGEventTapProxy, type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
    let pass = Unmanaged.passUnretained(event)
    switch type {
    case .tapDisabledByTimeout, .tapDisabledByUserInput:
        if let tap = ime.tap { CGEvent.tapEnable(tap: tap, enable: true) }
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
    sendReplacement(backspaces: Int(backspaces), text: text, proxy: proxy)
    return nil
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var toggleItem: NSMenuItem!
    var telexItem: NSMenuItem!
    var vniItem: NSMenuItem!
    var modernItem: NSMenuItem!
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
        menu.addItem(.separator())
        loginItem = add("Mở cùng máy", #selector(toggleLogin))
        if #unavailable(macOS 13) {
            loginItem.isHidden = true
        }
        menu.addItem(NSMenuItem(title: "Thoát", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        statusItem.menu = menu
        ime.onToggle = { [weak self] in self?.refresh() }
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
        let types: [CGEventType] = [.keyDown, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown]
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
    }

    func refresh() {
        statusItem.button?.title = ime.enabled ? "V" : "E"
        toggleItem.state = ime.enabled ? .on : .off
        telexItem.state = ime.vni ? .off : .on
        vniItem.state = ime.vni ? .on : .off
        modernItem.state = ime.modern ? .on : .off
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

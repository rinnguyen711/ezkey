// End-to-end check on a real desktop: starts ezkey.app and a target app, types
// Telex as HID keyboard events (through ezkey's event tap, like a physical
// keyboard) and reads the text back through the Accessibility API. Each case
// runs slow and with fast rollover (next key down before the previous key is
// up). Aborts if the target loses focus so no keys go astray.
//
//     sh macos/build.sh
//     swift macos/e2e.swift macos/build/ezkey.app [textedit|edge-bar|edge-page]
//
// edge-bar types into Microsoft Edge's address bar (inline autocomplete),
// edge-page into a text box on a page, read back through the clipboard (which
// is put back afterwards). Both open a new Edge tab and close it.
//
// The app running this (Terminal, VS Code, ...) needs Accessibility, and
// ezkey.app needs it too.

import Carbon.HIToolbox
import Cocoa

let cases: [(String, String)] = [
    ("tieengs vieetj", "tiếng việt"),
    ("xin chaof cacs banj", "xin chào các bạn"),
    ("dduwowngf", "đường"),
    ("nguowif", "người"),
    ("asd", "asd"),
    ("aa", "â"),
    ("af", "à"),
    ("of", "ò"),
    ("xin chafo", "xin chào"),
    ("goo", "gô"),
    ("ass aaa ww", "as aa w"),
    ("hello world", "hello world"),
    ("sorry", "sorry"),
    ("ko ddc", "không được"),
]

/// Milliseconds between key downs, and how long each key is held.
let profiles: [(name: String, gap: Double, hold: Double)] = [
    ("slow", 60, 20),
    ("normal", 120, 80),
    ("burst", 8, 30),
    ("fast", 15, 40),
]

let keycodes: [Character: Int] = [
    "a": kVK_ANSI_A, "b": kVK_ANSI_B, "c": kVK_ANSI_C, "d": kVK_ANSI_D, "e": kVK_ANSI_E,
    "f": kVK_ANSI_F, "g": kVK_ANSI_G, "h": kVK_ANSI_H, "i": kVK_ANSI_I, "j": kVK_ANSI_J,
    "k": kVK_ANSI_K, "l": kVK_ANSI_L, "m": kVK_ANSI_M, "n": kVK_ANSI_N, "o": kVK_ANSI_O,
    "p": kVK_ANSI_P, "q": kVK_ANSI_Q, "r": kVK_ANSI_R, "s": kVK_ANSI_S, "t": kVK_ANSI_T,
    "u": kVK_ANSI_U, "v": kVK_ANSI_V, "w": kVK_ANSI_W, "x": kVK_ANSI_X, "y": kVK_ANSI_Y,
    "z": kVK_ANSI_Z, " ": kVK_Space,
]

/// The user's ezkey settings, put back when the test ends.
let settings = UserDefaults(suiteName: "io.github.ezkey")!
let saved = ["Enabled", "Vni", "ModernTone"].map { ($0, settings.object(forKey: $0)) }
/// The user's abbreviation file, swapped for the test's and put back.
let abbreviations = FileManager.default
    .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    .appendingPathComponent("ezkey/abbreviations.txt")
let savedAbbreviations = try? Data(contentsOf: abbreviations)
try? FileManager.default.createDirectory(
    at: abbreviations.deletingLastPathComponent(), withIntermediateDirectories: true)
try! "ko=không\nddc=được\n".write(to: abbreviations, atomically: true, encoding: .utf8)

func restore() {
    for (key, value) in saved { settings.set(value, forKey: key) }
    if let savedAbbreviations {
        try? savedAbbreviations.write(to: abbreviations)
    } else {
        try? FileManager.default.removeItem(at: abbreviations)
    }
    command(false)
}

func fail(_ message: String) -> Never {
    restore()
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

/// The focused text field of the app, found through the Accessibility API.
func focusedField(_ pid: pid_t) -> AXUIElement {
    let app = AXUIElementCreateApplication(pid)
    // Chromium only builds its accessibility tree when asked.
    AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
    for _ in 0..<20 {
        var focused: CFTypeRef?
        if AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
           let element = focused {
            return element as! AXUIElement
        }
        sleep(ms: 250)
    }
    fail("cannot find the focused text field")
}

/// The field's text up to the selection: an address bar's selected
/// autocomplete suggestion is not part of what was typed.
func typedText(_ field: AXUIElement) -> String {
    var value: CFTypeRef?
    AXUIElementCopyAttributeValue(field, kAXValueAttribute as CFString, &value)
    let text = value as? String ?? ""
    var rangeValue: CFTypeRef?
    var range = CFRange()
    if AXUIElementCopyAttributeValue(field, kAXSelectedTextRangeAttribute as CFString, &rangeValue) == .success,
       AXValueGetValue(rangeValue as! AXValue, .cfRange, &range), range.length > 0 {
        return String(text.utf16.prefix(range.location)) ?? text
    }
    return text
}

func setText(_ field: AXUIElement, _ s: String) {
    AXUIElementSetAttributeValue(field, kAXValueAttribute as CFString, s as CFString)
}

func sleep(ms: Double) {
    usleep(useconds_t(ms * 1000))
}

var targetBundle = "com.apple.TextEdit"

func targetIsFront() -> Bool {
    NSWorkspace.shared.frontmostApplication?.bundleIdentifier == targetBundle
}

/// Command up or down, as its own event like a real keyboard sends it.
/// Only setting the Command flag on a key event leaves Command stuck down
/// system-wide after the test.
func command(_ down: Bool) {
    let e = CGEvent(keyboardEventSource: CGEventSource(stateID: .hidSystemState),
                    virtualKey: CGKeyCode(kVK_Command), keyDown: down)
    e?.type = .flagsChanged
    e?.flags = down ? .maskCommand : []
    e?.post(tap: .cghidEventTap)
}

/// Presses a key, with Command held when `cmd` is set.
func press(_ key: Int, cmd: Bool = false) {
    guard targetIsFront() else { fail("target app lost focus, stopping") }
    let source = CGEventSource(stateID: .hidSystemState)
    if cmd { command(true) }
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(key), keyDown: down)
        e?.flags = cmd ? .maskCommand : []
        e?.post(tap: .cghidEventTap)
    }
    if cmd { command(false) }
}

func type(_ text: String, gap: Double, hold: Double) {
    let source = CGEventSource(stateID: .hidSystemState)
    var events: [(at: Double, key: Int, down: Bool)] = []
    for (i, ch) in text.enumerated() {
        guard let key = keycodes[ch] else { fail("no keycode for \(ch)") }
        events.append((Double(i) * gap, key, true))
        events.append((Double(i) * gap + hold, key, false))
    }
    events.sort { $0.at < $1.at }
    var now = 0.0
    for e in events {
        sleep(ms: e.at - now)
        now = e.at
        guard targetIsFront() else { fail("target app lost focus, stopping") }
        CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(e.key), keyDown: e.down)?
            .post(tap: .cghidEventTap)
    }
}

let args = CommandLine.arguments
guard args.count == 2 || args.count == 3 else {
    fail("usage: swift macos/e2e.swift path/to/ezkey.app [textedit|edge-bar|edge-page]")
}
guard AXIsProcessTrusted() else { fail("give this terminal app Accessibility permission first") }
let app = args[1]
let target = args.count == 3 ? args[2] : "textedit"

// Start ezkey with Telex and Vietnamese on.
settings.set(true, forKey: "Enabled")
settings.set(false, forKey: "Vni")
settings.set(false, forKey: "ModernTone")
settings.synchronize()
let pkill = Process.launchedProcess(launchPath: "/usr/bin/pkill", arguments: ["-x", "ezkey"])
pkill.waitUntilExit()
sleep(ms: 500)
Process.launchedProcess(launchPath: "/usr/bin/open", arguments: [app]).waitUntilExit()
sleep(ms: 2000)

let open: [String]
switch target {
case "textedit":
    // A fresh empty file, so TextEdit opens a document and not its Open panel.
    let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("ezkey-e2e.txt")
    try! "".write(to: scratch, atomically: true, encoding: .utf8)
    open = ["-a", "TextEdit", scratch.path]
case "edge-bar":
    targetBundle = "com.microsoft.edgemac"
    open = ["-a", "Microsoft Edge", "about:blank"]
case "edge-page":
    targetBundle = "com.microsoft.edgemac"
    let page = FileManager.default.temporaryDirectory.appendingPathComponent("ezkey-e2e.html")
    try! "<input autofocus style='width:90%'>".write(to: page, atomically: true, encoding: .utf8)
    open = ["-a", "Microsoft Edge", page.absoluteString]
default:
    fail("unknown target \(target)")
}
Process.launchedProcess(launchPath: "/usr/bin/open", arguments: open).waitUntilExit()
sleep(ms: 2500)
guard targetIsFront(), let targetApp = NSWorkspace.shared.frontmostApplication else {
    fail("\(target) did not come to the front")
}

let pasteboard = NSPasteboard.general
let savedClipboard = pasteboard.string(forType: .string)

/// Empties the field; also ends ezkey's word (Cmd resets it).
func clearField() {
    switch target {
    case "textedit":
        setText(focusedField(targetApp.processIdentifier), "")
        return
    case "edge-bar":
        press(kVK_ANSI_L, cmd: true)
    default:
        press(kVK_ANSI_A, cmd: true)
    }
    press(kVK_Delete)
}

func readField() -> String {
    guard target == "edge-page" else { return typedText(focusedField(targetApp.processIdentifier)) }
    pasteboard.clearContents()
    press(kVK_ANSI_A, cmd: true)
    press(kVK_ANSI_C, cmd: true)
    sleep(ms: 300)
    return pasteboard.string(forType: .string) ?? ""
}

var failures = 0
for profile in profiles {
    for (input, expected) in cases {
        clearField()
        sleep(ms: 300)
        // The trailing space ends the word, so the next case starts fresh.
        type(input + " ", gap: profile.gap, hold: profile.hold)
        sleep(ms: 500)
        let got = String(readField().dropLast())
        // TextEdit capitalises the first letter; that is not ours to test.
        let ok = got.lowercased() == expected
        if !ok { failures += 1 }
        print("\(ok ? "ok  " : "FAIL") [\(profile.name)] \(input) -> \(got)\(ok ? "" : "  (want \(expected))")")
    }
}

if target == "textedit" {
    // Save the emptied scratch file so closing it asks nothing.
    setText(focusedField(targetApp.processIdentifier), "")
    press(kVK_ANSI_S, cmd: true)
    sleep(ms: 300)
}
press(kVK_ANSI_W, cmd: true)
if target == "edge-page" {
    pasteboard.clearContents()
    if let savedClipboard { pasteboard.setString(savedClipboard, forType: .string) }
}
restore()
print(failures == 0 ? "all passed" : "\(failures) failed")
exit(failures == 0 ? 0 : 1)

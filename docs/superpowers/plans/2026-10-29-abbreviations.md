# Abbreviation Dictionary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand abbreviations (`ko` → "không") at word end in V mode, keyed on the typed keys, with spell check on or off.

**Architecture:** The core `Engine` holds the dictionary and parses the file text, so both platforms share parsing and the expansion rule. A word-ending key (whitespace or ASCII punctuation) whose word's raw keys are in the dictionary returns `Replace { backspaces: displayed.len(), text: expansion + key }`. The platform shells only load the file, reload it when its mtime changes, and add a menu item.

**Tech Stack:** Rust (`ezkey-core`), Swift (`macos/main.swift`), Rust + Win32 (`windows/src/main.rs`).

Spec: `docs/superpowers/specs/2026-10-29-abbreviations-design.md`.

---

### Task 1: Core expansion

**Files:**
- Modify: `core/src/engine.rs` (struct `Engine`, `Engine::new`, `process_key`, tests)

- [ ] **Step 1: Write the failing test** (in `mod tests`)

```rust
    fn abbr(spell_check: bool, input: &str) -> String {
        let mut e = Engine::new();
        e.set_spell_check(spell_check);
        e.set_abbreviations("# comment\n\nko=không\nddc = được\nvn=Việt Nam\ntest=test\nbad line\n=x\nk o=y\n");
        run(e, input)
    }

    #[test]
    fn expands_abbreviations_at_word_end() {
        check(
            &[
                ("ko ", "không "),
                ("Ko,", "Không,"),
                ("vn.", "Việt Nam."),
                ("ddc ", "được "),
                ("test ", "test "),
                ("ko", "ko"),
                ("kox ", "kõ "),
                ("ko<o ", "ko "),
                ("koo ", "kô "),
                ("k o ", "k o "),
            ],
            |s| abbr(true, s),
        );
        assert_eq!(abbr(false, "ddc "), "được ");
    }
```

- [ ] **Step 2: Run it, expect a compile error (`set_abbreviations` missing)**

Run: `cargo test -q -p ezkey-core expands_abbreviations`

- [ ] **Step 3: Implement**

Field on `Engine` (init `HashMap::new()` in `new`, `use std::collections::HashMap;`):

```rust
    /// Typed keys → text, expanded at word end (`ko` → "không").
    abbreviations: HashMap<String, String>,
```

Methods:

```rust
    /// Loads `key=value` lines; `#` comments, blank and malformed lines are
    /// skipped. Keys are letters/digits, matched case-insensitively.
    pub fn set_abbreviations(&mut self, text: &str) {
        self.abbreviations = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .filter(|(k, v)| {
                !k.is_empty()
                    && k.chars().all(|c| c.is_ascii_alphanumeric())
                    && !v.is_empty()
                    && v.chars().count() <= 200
            })
            .collect();
    }

    /// The expansion of the word typed so far; `Ko` capitalises it.
    fn expansion(&self) -> Option<String> {
        let raw = self.raw.as_ref()?;
        let typed: String = raw.iter().collect();
        let value = self.abbreviations.get(&typed.to_ascii_lowercase())?;
        let mut chars = value.chars();
        Some(match chars.next() {
            Some(first) if raw[0].is_ascii_uppercase() => first.to_uppercase().chain(chars).collect(),
            _ => value.clone(),
        })
    }
```

In `process_key`, replace the non-letter early return:

```rust
        if !key.is_ascii_alphabetic() && !vni_digit {
            let ends_word = key.is_whitespace() || key.is_ascii_punctuation();
            let expansion = self.expansion().filter(|_| ends_word);
            let backspaces = self.displayed.len();
            self.reset();
            return match expansion {
                Some(text) => Action::Replace { backspaces, text: format!("{text}{key}") },
                None => Action::PassThrough,
            };
        }
```

- [ ] **Step 4: Run all core tests, expect PASS**

Run: `cargo test -q -p ezkey-core`

- [ ] **Step 5: Commit** — "Expand abbreviations at word end in the engine"

### Task 2: FFI

**Files:** `core/src/ffi.rs`, `core/include/ezkey.h`

- [ ] Add, after `ezkey_engine_set_spell_check`:

```rust
/// Loads the abbreviation file's text (UTF-8, `key=value` lines).
///
/// # Safety
/// `engine` must be valid; `text` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_set_abbreviations(engine: *mut Engine, text: *const c_char) {
    let text = CStr::from_ptr(text).to_string_lossy();
    (*engine).set_abbreviations(&text);
}
```

with `use std::ffi::{c_char, CStr};`, and in `ezkey.h`:

```c
/* Abbreviation file text, UTF-8 "key=value" lines: ko=không */
void ezkey_engine_set_abbreviations(EzkeyEngine *engine, const char *text);
```

- [ ] `cargo build -q -p ezkey-core`, commit — "Expose abbreviations through the C API"

### Task 3: macOS

**Files:** `macos/main.swift`, `macos/e2e.swift`, `README.md`

- [ ] In `handle`, raise `out` from 64 to 256 code points.
- [ ] Add to the IME class:

```swift
    static let abbreviationsURL = FileManager.default
        .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("ezkey/abbreviations.txt")
    var abbreviationsDate: Date?

    /// Reloads the abbreviation file when it changed; cheap enough per word.
    func loadAbbreviations() {
        let url = IME.abbreviationsURL
        let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        guard date != abbreviationsDate else { return }
        abbreviationsDate = date
        ezkey_engine_set_abbreviations(engine, (try? String(contentsOf: url, encoding: .utf8)) ?? "")
    }

    @objc func editAbbreviations() {
        let url = IME.abbreviationsURL
        if !FileManager.default.fileExists(atPath: url.path) {
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? "# ezkey: moi dong mot tu, dang tat=day du\nko=không\nddc=được\n".write(to: url, atomically: true, encoding: .utf8)
        }
        NSWorkspace.shared.open(url)
    }
```

  (Match the class/property names actually used in `main.swift`.) Call `loadAbbreviations()` at startup and in `handle` just before `ezkey_engine_process` when the key is not a letter or digit.
- [ ] Menu item "Sửa từ điển viết tắt…" → `editAbbreviations`.
- [ ] `macos/e2e.swift`: write a temp dictionary with `ko=không`, type `ko ` in TextEdit, expect "không ". Verify Enter/Tab reach the engine as `\r`/`\t`.
- [ ] README: document the feature under "Chung". Build with `macos/build.sh`; the user runs e2e (needs Accessibility). Commit — "Add an abbreviation dictionary on macOS"

### Task 4: Windows

**Files:** `windows/src/main.rs`, `README.md`

- [ ] In `on_key`, for keys that are not letters/digits, get the character with `ToUnicode` (current keyboard state); if it is whitespace or ASCII punctuation (plus `VK_RETURN` → `'\r'`, `VK_TAB` → `'\t'`), pass it to `process_key` and handle `Replace` like letters; otherwise `reset()` as today.
- [ ] Load `%APPDATA%\ezkey\abbreviations.txt` with the same mtime check; menu item "Sửa từ điển viết tắt…" creates the sample and opens it with `ShellExecuteW(.., "open", ..)`.
- [ ] `cargo build --release -p ezkey-win` on Windows (cross-check with `cargo check -p ezkey-win --target x86_64-pc-windows-msvc` here if the target is installed). Commit — "Add an abbreviation dictionary on Windows"

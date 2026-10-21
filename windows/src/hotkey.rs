//! The Vietnamese / English toggle hotkey: what it is, how it is stored, and
//! matching it against keyboard-hook events, including recording a new one.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;
pub const WIN: u8 = 8;

/// Modifiers plus one key, or modifiers only (`key == 0`, fires when they
/// are released with nothing pressed in between, like Ctrl+Shift).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub mods: u8,
    pub key: VIRTUAL_KEY,
}

/// Marks the encoded form; 0, 1 and 2 are the fixed choices of earlier
/// versions (Ctrl+Shift, Alt+Z, none).
const ENCODED: u32 = 0x8000_0000;

impl Hotkey {
    pub const CTRL_SHIFT: Hotkey = Hotkey {
        mods: CTRL | SHIFT,
        key: 0,
    };
    pub const ALT_Z: Hotkey = Hotkey {
        mods: ALT,
        key: 0x5A,
    };
    pub const NONE: Hotkey = Hotkey { mods: 0, key: 0 };

    pub fn from_setting(value: Option<u32>) -> Hotkey {
        match value {
            Some(1) => Hotkey::ALT_Z,
            Some(2) => Hotkey::NONE,
            Some(v) if v & ENCODED != 0 => Hotkey {
                mods: (v & 0xF) as u8,
                key: ((v >> 8) & 0xFFFF) as VIRTUAL_KEY,
            },
            _ => Hotkey::CTRL_SHIFT,
        }
    }

    pub fn to_setting(self) -> u32 {
        ENCODED | self.mods as u32 | (self.key as u32) << 8
    }

    pub fn is_none(self) -> bool {
        self == Hotkey::NONE
    }

    /// Whether the combination can be a hotkey without getting in the way
    /// of typing (a lone letter or Shift+letter cannot).
    pub fn is_valid(self) -> bool {
        match (self.mods, self.key) {
            (mods, 0) => mods.count_ones() >= 2,
            (0, key) => usable_alone(key),
            (SHIFT, key) => !types_text(key),
            _ => true,
        }
    }

    pub fn label(self) -> String {
        if self.is_none() {
            return "Không dùng phím tắt".into();
        }
        let mut parts: Vec<String> = [(CTRL, "Ctrl"), (SHIFT, "Shift"), (ALT, "Alt"), (WIN, "Win")]
            .into_iter()
            .filter(|(bit, _)| self.mods & bit != 0)
            .map(|(_, name)| name.to_string())
            .collect();
        if self.key != 0 {
            parts.push(key_name(self.key));
        }
        parts.join("+")
    }
}

pub fn modifier_bit(vk: VIRTUAL_KEY) -> Option<u8> {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(CTRL),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(SHIFT),
        VK_MENU | VK_LMENU | VK_RMENU => Some(ALT),
        VK_LWIN | VK_RWIN => Some(WIN),
        _ => None,
    }
}

fn types_text(vk: VIRTUAL_KEY) -> bool {
    matches!(vk, 0x30..=0x39 | 0x41..=0x5A | VK_SPACE | VK_NUMPAD0..=VK_NUMPAD9)
        || matches!(vk, VK_OEM_1..=VK_OEM_3 | VK_OEM_4..=VK_OEM_8 | VK_OEM_102)
}

fn usable_alone(vk: VIRTUAL_KEY) -> bool {
    matches!(vk, VK_F1..=VK_F24 | VK_PAUSE | VK_SCROLL | VK_CAPITAL)
}

fn key_name(vk: VIRTUAL_KEY) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        VK_F1..=VK_F24 => format!("F{}", vk - VK_F1 + 1),
        VK_SPACE => "Space".into(),
        VK_TAB => "Tab".into(),
        VK_OEM_3 => "`".into(),
        VK_CAPITAL => "Caps Lock".into(),
        VK_PAUSE => "Pause".into(),
        VK_SCROLL => "Scroll Lock".into(),
        _ => system_key_name(vk).unwrap_or_else(|| format!("0x{vk:02X}")),
    }
}

/// The keyboard layout's own name for a key, e.g. "Num 5" or "Insert".
fn system_key_name(vk: VIRTUAL_KEY) -> Option<String> {
    const EXTENDED: &[VIRTUAL_KEY] = &[
        VK_INSERT, VK_DELETE, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_LEFT, VK_RIGHT, VK_UP,
        VK_DOWN, VK_DIVIDE, VK_NUMLOCK, VK_APPS,
    ];
    unsafe {
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);
        if scan == 0 {
            return None;
        }
        let mut lparam = (scan as i32) << 16;
        if EXTENDED.contains(&vk) {
            lparam |= 1 << 24;
        }
        let mut buf = [0u16; 32];
        let len = GetKeyNameTextW(lparam, buf.as_mut_ptr(), buf.len() as i32);
        (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Not ours; let the key through.
    Pass,
    /// Toggle the mode. `swallow` is false for a modifier release, which the
    /// application must still see.
    Toggle { swallow: bool },
    /// Swallow: repeats and release of a key we already acted on.
    Swallow,
    /// Recording finished with a new hotkey.
    Recorded(Hotkey),
    /// Recording saw a combination that would break typing; still recording.
    Rejected,
    /// Recording cancelled with Esc.
    Cancelled,
}

/// Follows modifier state from hook events (GetAsyncKeyState lags behind
/// inside a low-level hook) and reports hotkey presses.
pub struct Matcher {
    pub hotkey: Hotkey,
    held: u8,
    /// Exactly the hotkey's modifiers are down and nothing else was pressed.
    armed: bool,
    /// A non-modifier key whose repeats and release we swallow.
    swallowing: Option<VIRTUAL_KEY>,
    recording: bool,
    /// Every modifier held at once since recording started.
    peak: u8,
}

impl Matcher {
    pub fn new(hotkey: Hotkey) -> Matcher {
        Matcher {
            hotkey,
            held: 0,
            armed: false,
            swallowing: None,
            recording: false,
            peak: 0,
        }
    }

    pub fn set(&mut self, hotkey: Hotkey) {
        self.hotkey = hotkey;
        self.armed = false;
        self.recording = false;
    }

    pub fn held(&self) -> u8 {
        self.held
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    pub fn start_recording(&mut self) {
        self.recording = true;
        self.armed = false;
        self.peak = 0;
    }

    pub fn cancel_recording(&mut self) {
        self.recording = false;
    }

    pub fn on_key(&mut self, vk: VIRTUAL_KEY, down: bool) -> Event {
        if let Some(bit) = modifier_bit(vk) {
            return self.on_modifier(bit, down);
        }
        if self.swallowing == Some(vk) {
            if !down {
                self.swallowing = None;
            }
            return Event::Swallow;
        }
        if !down {
            return Event::Pass;
        }
        self.armed = false;
        if self.recording {
            return self.record_key(vk);
        }
        if self.hotkey.key != 0 && vk == self.hotkey.key && self.held == self.hotkey.mods {
            self.swallowing = Some(vk);
            return Event::Toggle { swallow: true };
        }
        Event::Pass
    }

    fn on_modifier(&mut self, bit: u8, down: bool) -> Event {
        if down {
            self.held |= bit;
            if self.recording {
                self.peak |= self.held;
            } else {
                self.armed =
                    self.hotkey.key == 0 && !self.hotkey.is_none() && self.held == self.hotkey.mods;
            }
            return Event::Pass;
        }
        self.held &= !bit;
        if self.recording {
            if self.held == 0 && self.peak != 0 {
                let candidate = Hotkey {
                    mods: std::mem::take(&mut self.peak),
                    key: 0,
                };
                // A single modifier on its own: keep waiting.
                if candidate.is_valid() {
                    self.recording = false;
                    return Event::Recorded(candidate);
                }
            }
            return Event::Pass;
        }
        if std::mem::take(&mut self.armed) {
            return Event::Toggle { swallow: false };
        }
        Event::Pass
    }

    fn record_key(&mut self, vk: VIRTUAL_KEY) -> Event {
        self.swallowing = Some(vk);
        if vk == VK_ESCAPE && self.held == 0 {
            self.recording = false;
            return Event::Cancelled;
        }
        let candidate = Hotkey {
            mods: self.held,
            key: vk,
        };
        if !candidate.is_valid() {
            self.peak = 0;
            return Event::Rejected;
        }
        self.recording = false;
        Event::Recorded(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds (vk, down) events and collects every non-Pass result.
    fn run(m: &mut Matcher, events: &[(VIRTUAL_KEY, bool)]) -> Vec<Event> {
        events
            .iter()
            .map(|&(vk, down)| m.on_key(vk, down))
            .filter(|e| *e != Event::Pass)
            .collect()
    }

    const CTRL_DOWN: (VIRTUAL_KEY, bool) = (VK_LCONTROL, true);
    const CTRL_UP: (VIRTUAL_KEY, bool) = (VK_LCONTROL, false);
    const SHIFT_DOWN: (VIRTUAL_KEY, bool) = (VK_LSHIFT, true);
    const SHIFT_UP: (VIRTUAL_KEY, bool) = (VK_LSHIFT, false);
    const ALT_DOWN: (VIRTUAL_KEY, bool) = (VK_LMENU, true);
    const ALT_UP: (VIRTUAL_KEY, bool) = (VK_LMENU, false);

    #[test]
    fn ctrl_shift_fires_on_release() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        let got = run(&mut m, &[CTRL_DOWN, SHIFT_DOWN, SHIFT_UP, CTRL_UP]);
        assert_eq!(got, [Event::Toggle { swallow: false }]);
    }

    #[test]
    fn ctrl_shift_with_another_key_does_not_fire() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        let got = run(
            &mut m,
            &[
                CTRL_DOWN,
                SHIFT_DOWN,
                (0x54, true),
                (0x54, false),
                SHIFT_UP,
                CTRL_UP,
            ],
        );
        assert_eq!(got, []);
        let got = run(
            &mut m,
            &[CTRL_DOWN, SHIFT_DOWN, ALT_DOWN, ALT_UP, SHIFT_UP, CTRL_UP],
        );
        assert_eq!(got, []);
    }

    #[test]
    fn key_hotkey_swallows_press_repeats_and_release() {
        let mut m = Matcher::new(Hotkey::ALT_Z);
        let got = run(
            &mut m,
            &[ALT_DOWN, (0x5A, true), (0x5A, true), (0x5A, false), ALT_UP],
        );
        assert_eq!(
            got,
            [
                Event::Toggle { swallow: true },
                Event::Swallow,
                Event::Swallow
            ]
        );
        // Plain Z and Ctrl+Alt+Z are not the hotkey.
        assert_eq!(run(&mut m, &[(0x5A, true), (0x5A, false)]), []);
        assert_eq!(
            run(
                &mut m,
                &[
                    CTRL_DOWN,
                    ALT_DOWN,
                    (0x5A, true),
                    (0x5A, false),
                    ALT_UP,
                    CTRL_UP
                ]
            ),
            []
        );
    }

    #[test]
    fn none_never_fires() {
        let mut m = Matcher::new(Hotkey::NONE);
        assert_eq!(run(&mut m, &[CTRL_DOWN, SHIFT_DOWN, SHIFT_UP, CTRL_UP]), []);
    }

    #[test]
    fn records_key_combination() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        m.start_recording();
        let got = run(
            &mut m,
            &[CTRL_DOWN, (VK_SPACE, true), (VK_SPACE, false), CTRL_UP],
        );
        let ctrl_space = Hotkey {
            mods: CTRL,
            key: VK_SPACE,
        };
        assert_eq!(got, [Event::Recorded(ctrl_space), Event::Swallow]);
        assert!(!m.recording());
        m.set(ctrl_space);
        let got = run(
            &mut m,
            &[CTRL_DOWN, (VK_SPACE, true), (VK_SPACE, false), CTRL_UP],
        );
        assert_eq!(got, [Event::Toggle { swallow: true }, Event::Swallow]);
    }

    #[test]
    fn records_modifier_only_combination() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        m.start_recording();
        // A lone modifier is ignored; recording goes on.
        assert_eq!(run(&mut m, &[CTRL_DOWN, CTRL_UP]), []);
        let got = run(&mut m, &[CTRL_DOWN, ALT_DOWN, ALT_UP, CTRL_UP]);
        assert_eq!(
            got,
            [Event::Recorded(Hotkey {
                mods: CTRL | ALT,
                key: 0
            })]
        );
    }

    #[test]
    fn recording_rejects_typing_keys_and_cancels_on_escape() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        m.start_recording();
        assert_eq!(
            run(&mut m, &[(0x41, true), (0x41, false)]),
            [Event::Rejected, Event::Swallow]
        );
        let got = run(&mut m, &[SHIFT_DOWN, (0x41, true), (0x41, false), SHIFT_UP]);
        assert_eq!(got, [Event::Rejected, Event::Swallow]);
        assert!(m.recording());
        let got = run(&mut m, &[(VK_ESCAPE, true), (VK_ESCAPE, false)]);
        assert_eq!(got, [Event::Cancelled, Event::Swallow]);
        assert!(!m.recording());
        assert_eq!(m.hotkey, Hotkey::CTRL_SHIFT);
    }

    #[test]
    fn function_key_alone_is_allowed() {
        let mut m = Matcher::new(Hotkey::CTRL_SHIFT);
        m.start_recording();
        let got = run(&mut m, &[(VK_F9, true), (VK_F9, false)]);
        assert_eq!(
            got,
            [
                Event::Recorded(Hotkey {
                    mods: 0,
                    key: VK_F9
                }),
                Event::Swallow
            ]
        );
    }

    #[test]
    fn settings_round_trip_and_legacy_values() {
        assert_eq!(Hotkey::from_setting(None), Hotkey::CTRL_SHIFT);
        assert_eq!(Hotkey::from_setting(Some(0)), Hotkey::CTRL_SHIFT);
        assert_eq!(Hotkey::from_setting(Some(1)), Hotkey::ALT_Z);
        assert_eq!(Hotkey::from_setting(Some(2)), Hotkey::NONE);
        let custom = Hotkey {
            mods: CTRL | ALT | WIN,
            key: 0x56,
        };
        assert_eq!(Hotkey::from_setting(Some(custom.to_setting())), custom);
        assert_eq!(
            Hotkey::from_setting(Some(Hotkey::NONE.to_setting())),
            Hotkey::NONE
        );
    }

    #[test]
    fn labels() {
        assert_eq!(Hotkey::CTRL_SHIFT.label(), "Ctrl+Shift");
        assert_eq!(Hotkey::ALT_Z.label(), "Alt+Z");
        assert_eq!(
            Hotkey {
                mods: CTRL | ALT,
                key: VK_SPACE
            }
            .label(),
            "Ctrl+Alt+Space"
        );
        assert_eq!(
            Hotkey {
                mods: 0,
                key: VK_F12
            }
            .label(),
            "F12"
        );
    }
}

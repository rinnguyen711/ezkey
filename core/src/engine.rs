//! Telex / VNI state machine for the word currently being typed.
//!
//! The engine keeps the composed word as letters (with vowel marks) plus one
//! tone for the whole syllable; the tone's position is recomputed on every
//! render. Each key returns the edit needed to turn what is on screen into
//! the new rendering.

use std::collections::HashMap;

use crate::syllable::{
    circumflex, horn, is_horned, is_valid, parts, plain, split_tone, tone_position, with_tone,
    Tone, ToneStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Let the original key reach the application unchanged.
    PassThrough,
    /// Swallow the key, send `backspaces` backspaces, then type `text`.
    Replace { backspaces: usize, text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMethod {
    /// Letters as marks: `tieengs` → tiếng.
    Telex,
    /// Digits as marks: `tie6ng1` → tiếng.
    Vni,
}

#[derive(Clone, Copy, Debug)]
struct Letter {
    /// Lowercase, without tone (e.g. 'â', 'đ').
    ch: char,
    upper: bool,
    /// An 'ư' produced by a lone `w`; `ww` turns it back into "w".
    from_w: bool,
}

pub struct Engine {
    letters: Vec<Letter>,
    tone: Tone,
    /// Set after a transformation was undone (`ass` → "as"); the rest of the
    /// word is then typed literally.
    undone: bool,
    /// What the engine believes is on screen for the current word.
    displayed: Vec<char>,
    /// Keys typed for this word; restored when it stops being Vietnamese
    /// (`world` must not end up as "ươrld"). Unknown after a backspace.
    raw: Option<Vec<char>>,
    style: ToneStyle,
    method: InputMethod,
    /// Restore the typed keys when the word stops being Vietnamese. Off lets
    /// abbreviations like `ddc` → "đc" through, but `world` → "ưởld".
    spell_check: bool,
    /// Typed keys → text, expanded at word end (`ko` → "không").
    abbreviations: HashMap<String, String>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn to_upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            letters: Vec::new(),
            tone: Tone::None,
            undone: false,
            displayed: Vec::new(),
            raw: Some(Vec::new()),
            style: ToneStyle::Classic,
            method: InputMethod::Telex,
            spell_check: true,
            abbreviations: HashMap::new(),
        }
    }

    pub fn set_spell_check(&mut self, on: bool) {
        self.spell_check = on;
    }

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
            Some(first) if raw[0].is_ascii_uppercase() => {
                first.to_uppercase().chain(chars).collect()
            }
            _ => value.clone(),
        })
    }

    pub fn set_style(&mut self, style: ToneStyle) {
        self.style = style;
    }

    pub fn style(&self) -> ToneStyle {
        self.style
    }

    pub fn set_method(&mut self, method: InputMethod) {
        self.method = method;
        self.reset();
    }

    pub fn method(&self) -> InputMethod {
        self.method
    }

    /// Forget the current word (cursor moved, focus changed, word ended).
    pub fn reset(&mut self) {
        self.letters.clear();
        self.tone = Tone::None;
        self.undone = false;
        self.displayed.clear();
        self.raw = Some(Vec::new());
    }

    /// Handles a typed character. Non-letters end the current word, except
    /// VNI digits that apply to it.
    pub fn process_key(&mut self, key: char) -> Action {
        let vni_digit = self.method == InputMethod::Vni && key.is_ascii_digit();
        if !key.is_ascii_alphabetic() && !vni_digit {
            let ends_word = key.is_whitespace() || key.is_ascii_punctuation();
            let expansion = self.expansion().filter(|_| ends_word);
            let backspaces = self.displayed.len();
            self.reset();
            return match expansion {
                Some(text) => Action::Replace {
                    backspaces,
                    text: format!("{text}{key}"),
                },
                None => Action::PassThrough,
            };
        }
        if let Some(raw) = &mut self.raw {
            raw.push(key);
        }
        // `thiss` → "this" stays, but a new letter after the undo means a
        // doubled English letter (`sorry`, `issue`), so the word is checked
        // again. Repeating the undo key (`asss` → "ass") stays literal.
        let recheck = self.undone
            && self.raw.as_ref().is_some_and(|r| r.len() >= 2 && r[r.len() - 2] != key);
        if vni_digit {
            if self.undone || !self.apply_mark(key) {
                self.reset();
                return Action::PassThrough;
            }
        } else {
            self.apply(key);
        }
        if !self.undone || recheck {
            self.restore_if_invalid();
        }
        let new = self.render();
        let prefix = self
            .displayed
            .iter()
            .zip(&new)
            .take_while(|(a, b)| a == b)
            .count();
        let backspaces = self.displayed.len() - prefix;
        let text: String = new[prefix..].iter().collect();
        self.displayed = new;
        if backspaces == 0 && text.chars().eq(std::iter::once(key)) {
            Action::PassThrough
        } else {
            Action::Replace { backspaces, text }
        }
    }

    /// The application handles backspace itself; this keeps our copy in sync.
    pub fn backspace(&mut self) {
        if self.displayed.pop().is_none() {
            self.reset();
            return;
        }
        let shown = std::mem::take(&mut self.displayed);
        self.reset();
        for &c in &shown {
            let lower = c.to_lowercase().next().unwrap_or(c);
            let (base, tone) = split_tone(lower);
            if tone != Tone::None {
                self.tone = tone;
            }
            self.letters.push(Letter {
                ch: base,
                upper: lower != c,
                from_w: false,
            });
        }
        self.displayed = shown;
        self.raw = None;
    }

    /// Types the word back exactly as keyed once it can no longer be a
    /// Vietnamese syllable; the rest of the word stays literal.
    fn restore_if_invalid(&mut self) {
        let Some(raw) = self.raw.as_ref().filter(|_| self.spell_check) else { return };
        if is_valid(&self.chars(), false) || self.render() == *raw {
            return;
        }
        let raw = raw.clone();
        self.letters.clear();
        self.tone = Tone::None;
        for key in raw {
            self.push(key);
        }
        self.undone = true;
    }

    fn chars(&self) -> Vec<char> {
        self.letters.iter().map(|l| l.ch).collect()
    }

    fn render(&self) -> Vec<char> {
        let chars = self.chars();
        let pos = match self.tone {
            Tone::None => None,
            _ => tone_position(&chars, self.style),
        };
        self.letters
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let c = if Some(i) == pos {
                    with_tone(l.ch, self.tone)
                } else {
                    l.ch
                };
                if l.upper {
                    to_upper(c)
                } else {
                    c
                }
            })
            .collect()
    }

    fn apply(&mut self, key: char) {
        if !self.undone && self.apply_mark(key) {
            return;
        }
        self.push(key);
        if !self.undone {
            self.complete_uo();
        }
    }

    /// Applies `key` as a tone or vowel mark; false if it is just a letter.
    fn apply_mark(&mut self, key: char) -> bool {
        use InputMethod::{Telex, Vni};
        match (self.method, key.to_ascii_lowercase()) {
            (Telex, 's') | (Vni, '1') => self.try_tone(Tone::Sac, key),
            (Telex, 'f') | (Vni, '2') => self.try_tone(Tone::Huyen, key),
            (Telex, 'r') | (Vni, '3') => self.try_tone(Tone::Hoi, key),
            (Telex, 'x') | (Vni, '4') => self.try_tone(Tone::Nga, key),
            (Telex, 'j') | (Vni, '5') => self.try_tone(Tone::Nang, key),
            (Telex, 'z') | (Vni, '0') => self.try_clear_tone(),
            (Telex, k @ ('a' | 'e' | 'o')) => self.try_circumflex(Some(k), key),
            (Vni, '6') => self.try_circumflex(None, key),
            (Telex, 'w') => self.try_horn(&['a', 'o', 'u'], true, key),
            (Vni, '7') => self.try_horn(&['o', 'u'], false, key),
            (Vni, '8') => self.try_horn(&['a'], false, key),
            (Telex, 'd') | (Vni, '9') => self.try_stroke(key),
            _ => false,
        }
    }

    fn push(&mut self, key: char) {
        self.letters.push(Letter {
            ch: key.to_ascii_lowercase(),
            upper: key.is_ascii_uppercase(),
            from_w: false,
        });
    }

    /// Undo: drop the transformation and type the key literally.
    fn undo_with(&mut self, key: char) -> bool {
        self.push(key);
        self.undone = true;
        true
    }

    fn try_tone(&mut self, tone: Tone, key: char) -> bool {
        let chars = self.chars();
        let p = parts(&chars);
        if p.vowel_start == p.vowel_end || !is_valid(&chars, false) {
            return false;
        }
        if self.tone == tone {
            self.tone = Tone::None;
            return self.undo_with(key);
        }
        self.tone = tone;
        true
    }

    fn try_clear_tone(&mut self) -> bool {
        if self.tone == Tone::None {
            return false;
        }
        self.tone = Tone::None;
        true
    }

    /// `aa` → â, `ee` → ê, `oo` → ô, also delayed (`tien` + `e` → tiên).
    /// `vowel` is None for VNI `6`, which marks whichever of a/e/o is last.
    fn try_circumflex(&mut self, vowel: Option<char>, key: char) -> bool {
        let chars = self.chars();
        let p = parts(&chars);
        let Some(i) = (p.vowel_start..p.vowel_end).rev().find(|&i| match vowel {
            Some(v) => plain(chars[i]) == v,
            None => matches!(plain(chars[i]), 'a' | 'e' | 'o'),
        }) else {
            return false;
        };
        if matches!(chars[i], 'â' | 'ê' | 'ô') {
            self.letters[i].ch = plain(chars[i]);
            return self.undo_with(key);
        }
        let Some(marked) = circumflex(chars[i]) else {
            return false;
        };
        let mut cand = chars;
        cand[i] = marked;
        if !is_valid(&cand, true) {
            return false;
        }
        self.letters[i].ch = marked;
        true
    }

    /// Adds a breve or horn to one of `targets` (plain vowels): aw → ă,
    /// ow → ơ, uw → ư, uow → ươ, and with `standalone` a lone w → ư.
    fn try_horn(&mut self, targets: &[char], standalone: bool, key: char) -> bool {
        let chars = self.chars();
        let p = parts(&chars);
        let (vs, ve) = (p.vowel_start, p.vowel_end);
        let target = |c: char| targets.contains(&plain(c));

        // "uo" takes the horn on both vowels once something follows it.
        for i in (vs..ve.saturating_sub(1)).filter(|_| targets.contains(&'o')) {
            let (a, b) = (chars[i], chars[i + 1]);
            if plain(a) == 'u' && plain(b) == 'o' && !(a == 'ư' && b == 'ơ') {
                let mut cand = chars.clone();
                cand[i + 1] = 'ơ';
                if a == 'ư' || i + 2 < chars.len() {
                    cand[i] = 'ư';
                }
                if is_valid(&cand, true) {
                    self.letters[i].ch = cand[i];
                    self.letters[i + 1].ch = cand[i + 1];
                    return true;
                }
            }
        }

        let horned = |c: char| is_horned(c) && target(c);
        if (vs..ve).any(|i| horned(chars[i])) {
            for i in (vs..ve).rev() {
                if self.letters[i].from_w {
                    self.letters.remove(i);
                } else if horned(self.letters[i].ch) {
                    self.letters[i].ch = plain(self.letters[i].ch);
                }
            }
            return self.undo_with(key);
        }

        for i in (vs..ve).rev().filter(|&i| target(chars[i])) {
            if let Some(marked) = horn(chars[i]) {
                let mut cand = chars.clone();
                cand[i] = marked;
                if is_valid(&cand, true) {
                    self.letters[i].ch = marked;
                    return true;
                }
            }
        }

        if standalone && vs == ve {
            let mut cand = chars;
            cand.push('ư');
            if is_valid(&cand, true) {
                self.letters.push(Letter {
                    ch: 'ư',
                    upper: key.is_ascii_uppercase(),
                    from_w: true,
                });
                return true;
            }
        }
        false
    }

    /// `dd` → đ, also delayed (`did` → đi).
    fn try_stroke(&mut self, key: char) -> bool {
        match self.letters.first().map(|l| l.ch) {
            Some('d') => {
                let mut cand = self.chars();
                cand[0] = 'đ';
                if !is_valid(&cand, true) {
                    return false;
                }
                self.letters[0].ch = 'đ';
                true
            }
            Some('đ') => {
                self.letters[0].ch = 'd';
                self.undo_with(key)
            }
            _ => false,
        }
    }

    /// "ưo" / "uơ" followed by more letters become "ươ" (duwocj → được).
    fn complete_uo(&mut self) {
        let chars = self.chars();
        let p = parts(&chars);
        for i in p.vowel_start..p.vowel_end.saturating_sub(1) {
            if i + 2 >= chars.len() {
                break;
            }
            match (chars[i], chars[i + 1]) {
                ('ư', 'o') => self.letters[i + 1].ch = 'ơ',
                ('u', 'ơ') => self.letters[i].ch = 'ư',
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates an application text field driven by the engine; `<` is backspace.
    fn type_with(style: ToneStyle, input: &str) -> String {
        let mut e = Engine::new();
        e.set_style(style);
        run(e, input)
    }

    #[test]
    fn spell_check_off_keeps_marks() {
        let mut e = Engine::new();
        e.set_spell_check(false);
        assert_eq!(run(e, "ddc vaayj"), "đc vậy");
        assert_eq!(t("ddc"), "ddc");
    }

    fn abbr(spell_check: bool, input: &str) -> String {
        let mut e = Engine::new();
        e.set_spell_check(spell_check);
        e.set_abbreviations(
            "# comment\n\nko=không\nddc = được\nvn=Việt Nam\ntest=test\nbad line\n=x\nk o=y\n",
        );
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

    fn vni(input: &str) -> String {
        let mut e = Engine::new();
        e.set_method(InputMethod::Vni);
        run(e, input)
    }

    fn run(mut e: Engine, input: &str) -> String {
        let mut screen: Vec<char> = Vec::new();
        for key in input.chars() {
            if key == '<' {
                e.backspace();
                screen.pop();
                continue;
            }
            match e.process_key(key) {
                Action::PassThrough => screen.push(key),
                Action::Replace { backspaces, text } => {
                    for _ in 0..backspaces {
                        screen.pop();
                    }
                    screen.extend(text.chars());
                }
            }
        }
        screen.into_iter().collect()
    }

    fn t(input: &str) -> String {
        type_with(ToneStyle::Classic, input)
    }

    fn check(cases: &[(&str, &str)], f: fn(&str) -> String) {
        let failures: Vec<String> = cases
            .iter()
            .filter_map(|(input, want)| {
                let got = f(input);
                (got != *want).then(|| format!("{input:?}: got {got:?}, want {want:?}"))
            })
            .collect();
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn vni_words() {
        check(
            &[
                ("tie6ng1 vie6t5", "tiếng việt"),
                ("Vie65t Nam", "Việt Nam"),
                ("d9u7o7ng2", "đường"),
                ("d9uo7ng2", "đường"),
                ("nguo7i2", "người"),
                ("hoa2", "hòa"),
                ("a8n", "ăn"),
                ("mu7a", "mưa"),
                ("quo6c1", "quốc"),
                ("a11", "a1"),
                ("a66", "a6"),
                ("a10", "a"),
                ("123 a1", "123 á"),
                ("abc1", "abc1"),
                ("as", "as"),
                ("d9i<", "đ"),
            ],
            vni,
        );
    }

    #[test]
    fn basic_words() {
        check(
            &[
                ("tieengs vieetj", "tiếng việt"),
                ("xin chaof", "xin chào"),
                ("Vieetj Nam", "Việt Nam"),
                ("VIEETJ NAM", "VIỆT NAM"),
                ("nguyeenx", "nguyễn"),
                ("dduwowngf", "đường"),
                ("dduwowcj", "được"),
                ("duwocj", "dược"),
                ("nguowif", "người"),
                ("tuwowi", "tươi"),
                ("chuyeenj", "chuyện"),
                ("khuyeens", "khuyến"),
                ("quoocs", "quốc"),
                ("quas", "quá"),
                ("gif", "gì"),
                ("giaf", "già"),
                ("giuwax", "giữa"),
                ("gieets", "giết"),
                ("toans", "toán"),
                ("hoafng", "hoàng"),
                ("muwa", "mưa"),
                ("muaw", "mưa"),
                ("xoawn", "xoăn"),
                ("thuowr", "thuở"),
                ("thuowng", "thương"),
                ("huwu", "hưu"),
                ("khuyur", "khuỷu"),
                ("ngoais", "ngoái"),
                ("bans", "bán"),
                ("ddi", "đi"),
                ("Ddaaf", "Đầ"),
                ("DDAAYF", "ĐẦY"),
                ("tw", "tư"),
                ("nhw", "như"),
                ("w", "ư"),
                ("W", "Ư"),
            ],
            t,
        );
    }

    #[test]
    fn delayed_marks() {
        check(
            &[
                ("tiengs", "tiéng"),
                ("tienge", "tiêng"),
                ("vietej", "việt"),
                ("tuongw", "tương"),
                ("nguoiwf", "người"),
                ("dongd", "đong"),
                ("hoaf", "hòa"),
                ("hoafn", "hoàn"),
            ],
            t,
        );
    }

    #[test]
    fn undo_and_tone_change() {
        check(
            &[
                ("ass", "as"),
                ("asss", "ass"),
                ("aaa", "aa"),
                ("aww", "aw"),
                ("ww", "w"),
                ("dd", "đ"),
                ("ddd", "dd"),
                ("asf", "à"),
                ("asz", "a"),
                ("az", "az"),
                ("uww", "uw"),
            ],
            t,
        );
    }

    #[test]
    fn leaves_non_vietnamese_alone() {
        check(
            &[
                ("class", "class"),
                ("sa", "sa"),
                ("fix", "fix"),
                ("hello world", "hello world"),
                ("were", "were"),
                ("windows", "windows"),
                ("1s", "1s"),
                ("sorry", "sorry"),
                ("issue", "issue"),
                ("access", "access"),
                ("ass", "as"),
                ("thiss", "this"),
            ],
            t,
        );
    }

    #[test]
    fn tone_styles() {
        check(&[("hoaf", "hòa"), ("thuys", "thúy"), ("khoer", "khỏe")], t);
        check(
            &[
                ("hoaf", "hoà"),
                ("thuys", "thuý"),
                ("khoer", "khoẻ"),
                ("hoafng", "hoàng"),
            ],
            |s| type_with(ToneStyle::Modern, s),
        );
    }

    #[test]
    fn backspace_keeps_state_in_sync() {
        check(
            &[
                ("hoafn<", "hoà"),
                ("hoafn<n", "hoàn"),
                ("tieengs<<", "tiế"),
                ("tieengs<<n", "tiến"),
                ("ass<s", "á"),
                ("ab<<", ""),
                ("<<as", "á"),
            ],
            t,
        );
    }

    #[test]
    fn replace_is_minimal() {
        let mut e = Engine::new();
        assert_eq!(e.process_key('v'), Action::PassThrough);
        assert_eq!(e.process_key('i'), Action::PassThrough);
        assert_eq!(e.process_key('e'), Action::PassThrough);
        assert_eq!(
            e.process_key('e'),
            Action::Replace {
                backspaces: 1,
                text: "ê".into()
            }
        );
        assert_eq!(e.process_key('t'), Action::PassThrough);
        assert_eq!(
            e.process_key('j'),
            Action::Replace {
                backspaces: 2,
                text: "ệt".into()
            }
        );
    }
}

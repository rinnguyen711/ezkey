//! Vietnamese letter tables and syllable structure rules.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
    #[default]
    None,
    Sac,
    Huyen,
    Hoi,
    Nga,
    Nang,
}

impl Tone {
    fn index(self) -> usize {
        self as usize
    }

    fn from_index(i: usize) -> Tone {
        [
            Tone::None,
            Tone::Sac,
            Tone::Huyen,
            Tone::Hoi,
            Tone::Nga,
            Tone::Nang,
        ][i]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneStyle {
    /// hòa, thúy, khỏe
    Classic,
    /// hoà, thuý, khoẻ
    Modern,
}

/// Base vowels and their forms for each tone, in `Tone` order.
const VOWELS: [(char, [char; 6]); 12] = [
    ('a', ['a', 'á', 'à', 'ả', 'ã', 'ạ']),
    ('ă', ['ă', 'ắ', 'ằ', 'ẳ', 'ẵ', 'ặ']),
    ('â', ['â', 'ấ', 'ầ', 'ẩ', 'ẫ', 'ậ']),
    ('e', ['e', 'é', 'è', 'ẻ', 'ẽ', 'ẹ']),
    ('ê', ['ê', 'ế', 'ề', 'ể', 'ễ', 'ệ']),
    ('i', ['i', 'í', 'ì', 'ỉ', 'ĩ', 'ị']),
    ('o', ['o', 'ó', 'ò', 'ỏ', 'õ', 'ọ']),
    ('ô', ['ô', 'ố', 'ồ', 'ổ', 'ỗ', 'ộ']),
    ('ơ', ['ơ', 'ớ', 'ờ', 'ở', 'ỡ', 'ợ']),
    ('u', ['u', 'ú', 'ù', 'ủ', 'ũ', 'ụ']),
    ('ư', ['ư', 'ứ', 'ừ', 'ử', 'ữ', 'ự']),
    ('y', ['y', 'ý', 'ỳ', 'ỷ', 'ỹ', 'ỵ']),
];

const INITIALS: [&str; 28] = [
    "ngh", "ng", "nh", "ch", "gh", "gi", "kh", "ph", "qu", "th", "tr", "b", "c", "d", "đ", "g",
    "h", "k", "l", "m", "n", "p", "q", "r", "s", "t", "v", "x",
];

const FINALS: [&str; 9] = ["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

/// Vowel nuclei that may appear in a syllable (or while one is being typed).
const CLUSTERS: [&str; 56] = [
    "a", "ă", "â", "e", "ê", "i", "o", "ô", "ơ", "u", "ư", "y", //
    "ai", "ao", "au", "ay", "âu", "ây", "eo", "êu", "ia", "iê", "iu", "oa", "oă", "oe", "oi", "ôi",
    "ơi", "oo", "ua", "uâ", "uê", "ui", "uô", "uơ", "uy", "ưa", "ưi", "ươ", "ưu", "yê", "iêu",
    "yêu", "oai", "oay", "oao", "oeo", "uây", "uôi", "ươi", "ươu", "uya", "uyê", "uyu", "uơi",
];

pub fn is_vowel(c: char) -> bool {
    VOWELS.iter().any(|(v, _)| *v == c)
}

/// Strips circumflex, breve, horn and stroke: â→a, ơ→o, đ→d.
pub fn plain(c: char) -> char {
    match c {
        'ă' | 'â' => 'a',
        'ê' => 'e',
        'ô' | 'ơ' => 'o',
        'ư' => 'u',
        'đ' => 'd',
        _ => c,
    }
}

pub fn circumflex(c: char) -> Option<char> {
    match c {
        'a' | 'ă' => Some('â'),
        'e' => Some('ê'),
        'o' | 'ơ' => Some('ô'),
        _ => None,
    }
}

pub fn horn(c: char) -> Option<char> {
    match c {
        'a' => Some('ă'),
        'o' => Some('ơ'),
        'u' => Some('ư'),
        _ => None,
    }
}

pub fn is_horned(c: char) -> bool {
    matches!(c, 'ă' | 'ơ' | 'ư')
}

pub fn with_tone(base: char, tone: Tone) -> char {
    VOWELS
        .iter()
        .find(|(v, _)| *v == base)
        .map_or(base, |(_, forms)| forms[tone.index()])
}

/// Splits a lowercase on-screen character into base letter and tone.
pub fn split_tone(c: char) -> (char, Tone) {
    for (base, forms) in VOWELS.iter() {
        if let Some(i) = forms.iter().position(|f| *f == c) {
            return (*base, Tone::from_index(i));
        }
    }
    (c, Tone::None)
}

/// `[0, vowel_start)` is the initial consonant, `[vowel_start, vowel_end)`
/// the vowel cluster, `[vowel_end, len)` the final consonant.
#[derive(Clone, Copy, Debug)]
pub struct Parts {
    pub vowel_start: usize,
    pub vowel_end: usize,
}

fn starts_with(chars: &[char], prefix: &str) -> bool {
    let n = prefix.chars().count();
    chars.len() >= n && prefix.chars().zip(chars).all(|(a, b)| a == *b)
}

fn eq(chars: &[char], s: &str) -> bool {
    chars.len() == s.chars().count() && s.chars().zip(chars).all(|(a, b)| a == *b)
}

fn initial_len(chars: &[char]) -> Option<usize> {
    for init in INITIALS {
        if !starts_with(chars, init) {
            continue;
        }
        let n = init.chars().count();
        // "gi" and "qu" only act as initials when another vowel follows.
        if (init == "gi" || init == "qu") && !chars.get(n).is_some_and(|c| is_vowel(*c)) {
            continue;
        }
        return Some(n);
    }
    None
}

/// Locates the vowel cluster. Never fails: unknown leading consonants are
/// treated as part of the initial so a tone can still be rendered.
pub fn parts(chars: &[char]) -> Parts {
    let start = initial_len(chars).unwrap_or_else(|| {
        chars
            .iter()
            .position(|c| is_vowel(*c))
            .unwrap_or(chars.len())
    });
    let end = start + chars[start..].iter().take_while(|c| is_vowel(**c)).count();
    Parts {
        vowel_start: start,
        vowel_end: end,
    }
}

fn cluster_ok(cluster: &[char], strict: bool) -> bool {
    if strict {
        CLUSTERS.iter().any(|c| eq(cluster, c))
    } else {
        let p: Vec<char> = cluster.iter().map(|c| plain(*c)).collect();
        CLUSTERS
            .iter()
            .any(|c| c.chars().map(plain).eq(p.iter().copied()))
    }
}

/// Whether `chars` (lowercase, no tone) is a Vietnamese syllable or a prefix
/// of one. `strict` requires exact vowel marks; otherwise marks are ignored
/// when checking the vowel cluster.
pub fn is_valid(chars: &[char], strict: bool) -> bool {
    let init = match initial_len(chars) {
        Some(n) => n,
        None if chars.first().is_some_and(|c| is_vowel(*c)) => 0,
        None => return false,
    };
    let p = parts(chars);
    if p.vowel_start != init {
        return false;
    }
    let cluster = &chars[p.vowel_start..p.vowel_end];
    let fin = &chars[p.vowel_end..];
    if cluster.is_empty() {
        return fin.is_empty();
    }
    cluster_ok(cluster, strict) && FINALS.iter().any(|f| eq(fin, f))
}

/// Index of the letter that carries the tone mark, if there is a vowel.
pub fn tone_position(chars: &[char], style: ToneStyle) -> Option<usize> {
    let p = parts(chars);
    let (vs, ve) = (p.vowel_start, p.vowel_end);
    let n = ve - vs;
    if n == 0 {
        return None;
    }
    // A vowel with a mark (ê, ơ, â, ...) always takes the tone; for ươ it is ơ.
    if let Some(i) = (vs..ve).rev().find(|&i| plain(chars[i]) != chars[i]) {
        return Some(i);
    }
    let has_final = ve < chars.len();
    Some(match n {
        1 => vs,
        2 if has_final => vs + 1,
        2 => {
            let pair = &chars[vs..ve];
            let modern_pair = eq(pair, "oa") || eq(pair, "oe") || eq(pair, "uy");
            if style == ToneStyle::Modern && modern_pair {
                vs + 1
            } else {
                vs
            }
        }
        _ => vs + 1,
    })
}

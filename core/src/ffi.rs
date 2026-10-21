//! C ABI for platform shells written in other languages (macOS / Swift).
//! See `core/include/ezkey.h`.

use crate::{Action, Engine, InputMethod, ToneStyle};

#[no_mangle]
pub extern "C" fn ezkey_engine_new() -> *mut Engine {
    Box::into_raw(Box::new(Engine::new()))
}

/// # Safety
/// `engine` must come from `ezkey_engine_new` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_free(engine: *mut Engine) {
    if !engine.is_null() {
        drop(Box::from_raw(engine));
    }
}

/// Processes one typed character (a Unicode scalar value).
///
/// Returns -1 when the key should pass through unchanged. Otherwise the key
/// must be swallowed, `*backspaces` backspaces sent, and the returned number
/// of UTF-32 code points from `out` typed.
///
/// # Safety
/// `engine` must be valid; `backspaces` must be writable; `out` must have
/// room for `out_cap` values.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_process(
    engine: *mut Engine,
    key: u32,
    backspaces: *mut u32,
    out: *mut u32,
    out_cap: u32,
) -> i32 {
    let engine = &mut *engine;
    let Some(key) = char::from_u32(key) else {
        engine.reset();
        return -1;
    };
    match engine.process_key(key) {
        Action::PassThrough => -1,
        Action::Replace {
            backspaces: n,
            text,
        } => {
            let chars: Vec<char> = text.chars().collect();
            if chars.len() > out_cap as usize {
                engine.reset();
                return -1;
            }
            for (i, c) in chars.iter().enumerate() {
                *out.add(i) = *c as u32;
            }
            *backspaces = n as u32;
            chars.len() as i32
        }
    }
}

/// # Safety
/// `engine` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_backspace(engine: *mut Engine) {
    (*engine).backspace();
}

/// # Safety
/// `engine` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_reset(engine: *mut Engine) {
    (*engine).reset();
}

/// # Safety
/// `engine` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_set_modern_style(engine: *mut Engine, modern: bool) {
    (*engine).set_style(if modern {
        ToneStyle::Modern
    } else {
        ToneStyle::Classic
    });
}

/// Switches between Telex (`vni == false`) and VNI.
///
/// # Safety
/// `engine` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ezkey_engine_set_vni(engine: *mut Engine, vni: bool) {
    (*engine).set_method(if vni {
        InputMethod::Vni
    } else {
        InputMethod::Telex
    });
}

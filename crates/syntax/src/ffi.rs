//! The C API of `shim.cpp` and a safe owner for a Lexilla lexer.

use std::{
    ffi::{CString, c_char, c_int, c_void},
    ptr::NonNull,
};

use crate::{Text, buffers::Buffers};

/// Callbacks the C++ document calls to reach the text and the buffers.
/// Mirrors `struct OxyDocument` in `shim.cpp`.
#[repr(C)]
struct OxyDocument {
    ctx: *mut c_void,
    length: isize,
    get_chars: unsafe extern "C" fn(*mut c_void, *mut c_char, isize, isize),
    style_at: unsafe extern "C" fn(*mut c_void, isize) -> c_char,
    line_from_position: unsafe extern "C" fn(*mut c_void, isize) -> isize,
    line_start: unsafe extern "C" fn(*mut c_void, isize) -> isize,
    get_level: unsafe extern "C" fn(*mut c_void, isize) -> c_int,
    set_level: unsafe extern "C" fn(*mut c_void, isize, c_int),
    get_line_state: unsafe extern "C" fn(*mut c_void, isize) -> c_int,
    set_line_state: unsafe extern "C" fn(*mut c_void, isize, c_int),
    set_styles: unsafe extern "C" fn(*mut c_void, isize, isize, *const c_char, c_char),
}

unsafe extern "C" {
    fn oxy_lexer_create(name: *const c_char) -> *mut c_void;
    fn oxy_lexer_release(lexer: *mut c_void);
    fn oxy_lexer_property_set(lexer: *mut c_void, key: *const c_char, value: *const c_char);
    fn oxy_lexer_word_list_set(lexer: *mut c_void, index: c_int, words: *const c_char);
    fn oxy_lexer_run(
        lexer: *mut c_void,
        document: *const OxyDocument,
        start: isize,
        length: isize,
        init_style: c_int,
    ) -> bool;
}

/// What the callbacks see: the text being lexed and where results go.
struct Session<'a> {
    text: &'a dyn Text,
    buffers: &'a mut Buffers,
}

/// A Lexilla lexer, released on drop.
pub(crate) struct Lexer(NonNull<c_void>);

impl Lexer {
    /// The lexer Lexilla knows as `name` ("cpp", "python", …).
    pub(crate) fn new(name: &str) -> Option<Self> {
        let name = CString::new(name).ok()?;
        // SAFETY: `name` is a valid C string for the duration of the call.
        NonNull::new(unsafe { oxy_lexer_create(name.as_ptr()) }).map(Self)
    }

    pub(crate) fn set_property(&mut self, key: &str, value: &str) {
        let (Ok(key), Ok(value)) = (CString::new(key), CString::new(value)) else { return };
        // SAFETY: the lexer is live and both strings outlive the call.
        unsafe { oxy_lexer_property_set(self.0.as_ptr(), key.as_ptr(), value.as_ptr()) }
    }

    pub(crate) fn set_keywords(&mut self, index: usize, words: &str) {
        let Ok(words) = CString::new(words) else { return };
        // SAFETY: the lexer is live and `words` outlives the call.
        unsafe { oxy_lexer_word_list_set(self.0.as_ptr(), index as c_int, words.as_ptr()) }
    }

    /// Lexes and folds `start..end` of `text`; `start` must be a line start.
    /// Returns false when the lexer failed.
    pub(crate) fn run(&mut self, text: &dyn Text, buffers: &mut Buffers, start: usize, end: usize) -> bool {
        let init_style = if start > 0 { buffers.styles.get(start - 1).copied().unwrap_or(0) } else { 0 };
        let mut session = Session { text, buffers };
        let document = OxyDocument {
            ctx: (&raw mut session).cast(),
            length: text.len() as isize,
            get_chars,
            style_at,
            line_from_position,
            line_start,
            get_level,
            set_level,
            get_line_state,
            set_line_state,
            set_styles,
        };
        // SAFETY: the lexer is live, and `document` and the session it points
        // to outlive the call; the callbacks only use them during it.
        unsafe {
            oxy_lexer_run(
                self.0.as_ptr(),
                &document,
                start as isize,
                (end - start) as isize,
                c_int::from(init_style),
            )
        }
    }
}

impl Drop for Lexer {
    fn drop(&mut self) {
        // SAFETY: the lexer is live and released exactly once.
        unsafe { oxy_lexer_release(self.0.as_ptr()) }
    }
}

// The callbacks below never panic: a panic cannot unwind into C++. Positions
// and lines from the lexer are clamped to the text.

/// # Safety
/// `ctx` must be the `Session` that `Lexer::run` passed.
unsafe fn session<'a>(ctx: *mut c_void) -> &'a mut Session<'a> {
    // SAFETY: guaranteed by the caller.
    unsafe { &mut *ctx.cast::<Session<'a>>() }
}

unsafe extern "C" fn get_chars(ctx: *mut c_void, buffer: *mut c_char, position: isize, length: isize) {
    // SAFETY: `ctx` comes from `Lexer::run`.
    let session = unsafe { session(ctx) };
    let position = position.max(0) as usize;
    let length = (length.max(0) as usize).min(session.text.len().saturating_sub(position));
    // SAFETY: the C++ side passes a buffer of at least `length` bytes.
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), length) };
    session.text.read(position, buffer);
}

unsafe extern "C" fn style_at(ctx: *mut c_void, position: isize) -> c_char {
    // SAFETY: `ctx` comes from `Lexer::run`.
    let session = unsafe { session(ctx) };
    session.buffers.styles.get(position.max(0) as usize).map_or(0, |&style| style as c_char)
}

unsafe extern "C" fn line_from_position(ctx: *mut c_void, position: isize) -> isize {
    // SAFETY: `ctx` comes from `Lexer::run`.
    let session = unsafe { session(ctx) };
    session.text.line_of((position.max(0) as usize).min(session.text.len())) as isize
}

unsafe extern "C" fn line_start(ctx: *mut c_void, line: isize) -> isize {
    // SAFETY: `ctx` comes from `Lexer::run`.
    let session = unsafe { session(ctx) };
    session.text.line_start(line.max(0) as usize) as isize
}

unsafe extern "C" fn get_level(ctx: *mut c_void, line: isize) -> c_int {
    // SAFETY: `ctx` comes from `Lexer::run`.
    unsafe { session(ctx) }.buffers.level(line.max(0) as usize)
}

unsafe extern "C" fn set_level(ctx: *mut c_void, line: isize, level: c_int) {
    // SAFETY: `ctx` comes from `Lexer::run`.
    unsafe { session(ctx) }.buffers.set_level(line.max(0) as usize, level);
}

unsafe extern "C" fn get_line_state(ctx: *mut c_void, line: isize) -> c_int {
    // SAFETY: `ctx` comes from `Lexer::run`.
    unsafe { session(ctx) }.buffers.line_state(line.max(0) as usize)
}

unsafe extern "C" fn set_line_state(ctx: *mut c_void, line: isize, state: c_int) {
    // SAFETY: `ctx` comes from `Lexer::run`.
    unsafe { session(ctx) }.buffers.set_line_state(line.max(0) as usize, state);
}

unsafe extern "C" fn set_styles(ctx: *mut c_void, position: isize, length: isize, styles: *const c_char, style: c_char) {
    // SAFETY: `ctx` comes from `Lexer::run`.
    let session = unsafe { session(ctx) };
    let total = session.buffers.styles.len();
    let start = (position.max(0) as usize).min(total);
    let end = start.saturating_add(length.max(0) as usize).min(total);
    let target = &mut session.buffers.styles[start..end];
    if styles.is_null() {
        target.fill(style as u8);
    } else {
        // SAFETY: the C++ side passes `length` styles; we read at most that.
        let source = unsafe { std::slice::from_raw_parts(styles.cast::<u8>(), target.len()) };
        target.copy_from_slice(source);
    }
}

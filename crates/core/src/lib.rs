//! Document model shared by every OxyPad front end.
//!
//! Nothing here depends on the UI toolkit, so it can be tested headless and
//! survive a change of UI framework.

mod compare;
mod document;
mod fs;
mod language;

pub use compare::{Alignment, Cell, Row, RowKind, align, lines};
pub use document::{Document, ExternalChange, LineEnding, TextEncoding};
pub use fs::{canonical_path, simplified, write_atomic};
pub use language::language_for_path;

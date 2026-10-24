//! Document model shared by every OxyPad front end.
//!
//! Nothing here depends on the UI toolkit, so it can be tested headless and
//! survive a change of UI framework.

mod comments;
mod compare;
mod document;
mod edit_ops;
mod fs;
mod language;
mod password;
mod text_transform;

pub use comments::{CommentMode, CommentSymbols, LineComments, Replacement, block_comment, block_uncomment, comment_lines, comment_symbols};
pub use compare::{Alignment, Cell, Row, RowKind, align, lines};
pub use edit_ops::{EditOp, EditPlan, EditProblem, EditSettings, plan_edit};
pub use document::{Document, ExternalChange, LineEnding, TextEncoding};
pub use fs::{canonical_path, simplified, write_atomic};
pub use language::language_for_path;
pub use password::{PasswordGenerator, PasswordProblem};
pub use text_transform::{
    Case, Sort, SortError, Trim, convert_case, is_blank, join_lines, remove_consecutive_duplicates,
    remove_duplicates, remove_empty_lines, reindent_line, sort_lines, spaces_to_tabs, tabs_to_spaces, trim_line,
};

//! Патчер «Умный Markdown» (Шаг 2 PRP): парсер тегов, каскад якорей,
//! двухфазный аплаер с бэкапом и Undo.
//!
//! Публичный API:
//! - [`parse`] — ответ LLM → [`ParsedResponse`];
//! - [`prepare`] — dry-run одного изменения (в памяти);
//! - [`apply`] — запись на диск (бэкап + «всё или ничего»);
//! - [`undo_last`] — откат последнего применения.

mod anchors;
mod applier;
mod models;
mod parser;
#[cfg(test)]
mod tests;

pub use anchors::AnchorError;
pub use applier::{apply, prepare, undo_last};
pub use models::{
    ApplyReport, ChangeKind, Edit, FileChange, FilePatchError, PatchError, ParsedResponse,
    PreparedChange, PreparedKind,
};
pub use parser::parse;

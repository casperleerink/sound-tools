//! The tree a window shows of the history of a project (`sound_core::history`): which points
//! and versions are rows, what folds away, and the lanes of the lines.

mod layout;

pub use layout::{Drawing, FOLD_AFTER, Fold, Graph, Layout, Line, Row, Shape, Version};

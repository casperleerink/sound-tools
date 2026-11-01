//! The history of a project: every state of its folder as a point, the versions they form,
//! and the tree a window shows of them. It knows no music and no project type: it looks at
//! files.

mod git;
mod layout;
mod store;

pub use git::{GitError, ObjectId};
pub use layout::{Drawing, FOLD_AFTER_MOMENTS, Fold, Graph, Layout, Line, Row, Shape, Version};
pub use store::{HISTORY_FOLDER, History, HistoryError, Point, Recorded, Scope};

//! The history of a project folder: every state it was in as a point, kept as git commits in
//! a repository inside the folder. It knows no music and no project type: it looks at files.

mod git;
mod store;

pub use git::{GitError, ObjectId};
pub use store::{FileChange, Files, HISTORY_FOLDER, History, HistoryError, Named, Point, Scope};

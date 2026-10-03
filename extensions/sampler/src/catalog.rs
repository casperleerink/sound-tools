//! The instruments of the library: free libraries on GitHub under CC0 or CC BY, each at a
//! pinned commit. Sizes are of the files an instrument's SFZ file needs, and of its samples
//! decoded in memory.

use crate::library::{Category, Entry, Library};

const VSCO: Library = Library {
    id: "vsco",
    name: "VSCO 2 Community Edition",
    repository: "sgossner/VSCO-2-CE",
    commit: "6dd651d55dde97fd4028699be9d4481f26917891",
    license: "CC0",
    attribution: None,
};

pub const CATALOG: &[Entry] = &[Entry {
    id: "vsco/cello-section-sustain",
    name: "Cello section, sustain",
    category: Category::Strings,
    library: &VSCO,
    sfz: "CelloEnsSusVib.sfz",
    download_bytes: 69_000_000,
    memory_bytes: 69_000_000,
}];

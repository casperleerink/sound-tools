use crate::Result;
use serde::Serialize;
use std::{fs, path::Path};

pub(crate) fn write(path: &Path, value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, &bytes)?;
    fs::rename(&temporary, path)?;
    Ok(bytes)
}

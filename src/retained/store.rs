use super::Entry;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::Path,
};

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub(super) fn transaction<T>(
    path: &Path,
    update: impl FnOnce(&mut BTreeMap<String, Entry>) -> Result<T, String>,
) -> Result<T, String> {
    let parent = path.parent().ok_or("state path has no parent")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path.with_extension("lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let _lock = Lock(lock);
    let mut db = BTreeMap::new();
    let before = match fs::read(path) {
        Ok(data) => Some(data),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if let Some(data) = &before {
        let v: Value =
            serde_json::from_slice(data).map_err(|e| format!("invalid session cache: {e}"))?;
        if v["version"].as_u64() != Some(1) {
            return Err("unsupported session cache version".into());
        }
        for row in v["entries"]
            .as_array()
            .ok_or("invalid session cache entries")?
        {
            let e = Entry::from_json(row).ok_or("invalid session cache record")?;
            db.insert(e.key.clone(), e);
        }
    }
    let output = update(&mut db)?;
    let bytes = serde_json::to_vec_pretty(
        &json!({"version":1,"entries":db.values().map(Entry::to_json).collect::<Vec<_>>()}),
    )
    .map_err(|e| e.to_string())?;
    if before.as_deref() != Some(bytes.as_slice()) {
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
    }
    Ok(output)
}

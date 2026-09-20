//! Local operational request journal. Scientific facts never live here.
use crate::{Error, Result, corpus::Corpus};
use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

pub struct Journal {
    root: PathBuf,
    _lock: File,
}
impl Corpus {
    pub fn operation_journal(&self) -> Result<Journal> {
        let root = PathBuf::from(self.git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?)
        .join("orbit-research-operations");
        fs::create_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("lock"))?;
        lock.lock_exclusive()?;
        Ok(Journal { root, _lock: lock })
    }
}
impl Journal {
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Invalid("Invalid operation key".into()));
        }
        Ok(self.root.join(format!("{key}.json")))
    }
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match fs::read(self.path(key)?) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        temp.write_all(&serde_json::to_vec_pretty(value)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.path(key)?)
            .map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
    pub fn list<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        let mut items = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                items.push(serde_json::from_slice(&fs::read(path)?)?);
            }
        }
        Ok(items)
    }
}

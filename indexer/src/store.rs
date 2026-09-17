//! Short-lived master access and exclusive, UUID-independent per-index databases.
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use bincode::{Decode, Encode};

use crate::db::{DbPrefix, IndexInfo, IndexInfoKey, LAYOUT_VERSION, ObjectState};

#[derive(Debug, Encode, Decode)]
struct Entry {
    info: IndexInfo,
    folder: u64,
}

pub struct Store {
    root: PathBuf,
}

fn is_locked(error: &sled::Error) -> bool {
    // sled 0.34 wraps file-lock failures in an Other error, losing the original kind.
    matches!(error, sled::Error::Io(e) if e.kind() == std::io::ErrorKind::WouldBlock
        || (e.to_string().starts_with("could not acquire lock on ")
            && e.to_string().contains("WouldBlock")))
}

/// Stamps a database this binary has just created, and refuses one written in any other
/// layout.
///
/// Every database in the store goes through here as it is opened, so a store written by a
/// binary that disagrees about the encodings is rejected before anything reads it.
fn check_layout_version(db: &sled::Db) -> anyhow::Result<()> {
    if !db.was_recovered() {
        db.insert(DbPrefix::Version.key(), &LAYOUT_VERSION.to_be_bytes())?;
        db.flush()?;
        return Ok(());
    }
    let header = db
        .get(DbPrefix::Version.key())?
        .context("database has no layout version header")?;
    let header: [u8; 4] = header
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("malformed layout version header"))?;
    let found = u32::from_be_bytes(header);
    anyhow::ensure!(
        found == LAYOUT_VERSION,
        "database is written in layout version {found}, this binary requires {LAYOUT_VERSION}"
    );
    Ok(())
}

fn encode<T: Encode>(value: &T) -> anyhow::Result<Vec<u8>> {
    Ok(bincode::encode_to_vec(value, bincode::config::standard())?)
}

fn decode<T: Decode<()>>(value: &[u8]) -> anyhow::Result<T> {
    let (decoded, consumed) = bincode::decode_from_slice(value, bincode::config::standard())?;
    anyhow::ensure!(consumed == value.len(), "trailing bytes in database record");
    Ok(decoded)
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates the store directory if it is not there yet, and refuses a path that is not a
    /// directory.
    fn ensure_root(&self) -> anyhow::Result<()> {
        match self.root.metadata() {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir(),
                "records path is not a directory: {}",
                self.root.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(&self.root).with_context(|| {
                    format!("cannot create records directory: {}", self.root.display())
                })?;
                tracing::info!(path = %self.root.display(), "created records directory");
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("cannot read records directory: {}", self.root.display())
                });
            }
        }
        Ok(())
    }

    /// Create, version-check and lock-check the store before commands perform any remote side
    /// effects.
    pub fn initialize(&self) -> anyhow::Result<()> {
        drop(self.master()?);
        Ok(())
    }

    fn master(&self) -> anyhow::Result<sled::Db> {
        self.ensure_root()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let master = loop {
            match sled::open(self.root.join("master")) {
                Ok(db) => break db,
                Err(e) if is_locked(&e) && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(e).context(
                        "cannot open master database (another command may be busy); retry the \
                         command",
                    );
                }
            }
        };
        check_layout_version(&master)?;
        Ok(master)
    }

    fn folder(&self, id: u64) -> PathBuf {
        self.root.join("indices").join(id.to_string())
    }

    fn entry(master: &sled::Db, uuid: &str) -> anyhow::Result<Entry> {
        let key = IndexInfoKey::new(uuid.to_owned())?;
        let value = master
            .get(key)?
            .with_context(|| format!("index not found: {uuid}"))?;
        decode(&value)
    }

    fn open_folder(&self, entry: &Entry) -> anyhow::Result<sled::Db> {
        let path = self.folder(entry.folder);
        anyhow::ensure!(
            path.join("db").exists(),
            "index database missing: {}",
            path.display()
        );
        match sled::open(path) {
            Ok(db) => {
                check_layout_version(&db)?;
                Ok(db)
            }
            Err(e) if is_locked(&e) => bail!(
                "index {} is busy (another command holds its database)",
                entry.info.uuid
            ),
            Err(e) => Err(e.into()),
        }
    }

    pub fn create(&self, info: IndexInfo) -> anyhow::Result<()> {
        let key = IndexInfoKey::new(info.uuid.clone())?;
        let master = self.master()?;
        anyhow::ensure!(
            !master.contains_key(&key)?,
            "UUID already in use: {}",
            info.uuid
        );
        let folder = master.generate_id()?;
        let index = sled::open(self.folder(folder))?;
        check_layout_version(&index)?;
        master.insert(key, encode(&Entry { info, folder })?)?;
        master.flush()?;
        Ok(())
    }

    pub fn open_index(&self, uuid: &str) -> anyhow::Result<(IndexInfo, sled::Db)> {
        let master = self.master()?;
        let entry = Self::entry(&master, uuid)?;
        let index = self.open_folder(&entry)?;
        // Acquire the index lock before dropping the master: rekey cannot race lookup.
        drop(master);
        Ok((entry.info, index))
    }

    pub fn rekey(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let to_key = IndexInfoKey::new(to.to_owned())?;
        let master = self.master()?;
        let mut entry = Self::entry(&master, from)?;
        let _index = self.open_folder(&entry)?;
        if from == to {
            return Ok(());
        }
        anyhow::ensure!(!master.contains_key(&to_key)?, "UUID already in use: {to}");
        entry.info.uuid = to.to_owned();
        let mut batch = sled::Batch::default();
        batch.remove(IndexInfoKey::new(from.to_owned())?.as_ref());
        batch.insert(to_key.as_ref(), encode(&entry)?);
        master.apply_batch(batch)?;
        master.flush()?;
        Ok(())
    }

    /// Reads an index's record and confirms no other command holds its database.
    ///
    /// Callers that are about to act on the node use this first, so that an index that cannot
    /// be removed from the records is not removed from the node either.
    pub fn index_info(&self, uuid: &str) -> anyhow::Result<IndexInfo> {
        let master = self.master()?;
        let entry = Self::entry(&master, uuid)?;
        drop(self.open_folder(&entry)?);
        Ok(entry.info)
    }

    /// Drops an index's record and the database holding its progress.
    ///
    /// The mapping is removed before the folder, so an interrupted delete leaves an
    /// unreferenced folder rather than a record pointing at a database that is gone.
    pub fn delete(&self, uuid: &str) -> anyhow::Result<IndexInfo> {
        let master = self.master()?;
        let entry = Self::entry(&master, uuid)?;
        let index = self.open_folder(&entry)?;
        master.remove(IndexInfoKey::new(uuid.to_owned())?)?;
        master.flush()?;
        // Drop index so that the folder can be deleted. Master lock is still held until end of
        // function.
        drop(index);
        std::fs::remove_dir_all(self.folder(entry.folder))?;
        Ok(entry.info)
    }

    pub fn list(&self) -> anyhow::Result<Vec<(IndexInfo, Option<u64>)>> {
        let master = self.master()?;
        let mut result = Vec::new();
        for item in master.scan_prefix(DbPrefix::IndexInfo.key()) {
            let (_, value) = item?;
            let entry: Entry = decode(&value)?;
            let path = self.folder(entry.folder);
            anyhow::ensure!(
                path.join("db").exists(),
                "index database missing: {}",
                path.display()
            );
            let count = match sled::open(path) {
                Ok(index) => {
                    check_layout_version(&index)?;
                    Some(index)
                }
                Err(e) if is_locked(&e) => None,
                Err(e) => return Err(e.into()),
            };
            result.push((entry.info, count));
        }
        drop(master);
        result
            .into_iter()
            .map(|(info, index)| Ok((info, index.as_ref().map(document_count).transpose()?)))
            .collect()
    }
}

fn document_count(index: &sled::Db) -> anyhow::Result<u64> {
    let mut count = 0u64;
    for item in index.iter() {
        let (key, value) = item?;
        if key.as_ref() == DbPrefix::Version.key() {
            continue;
        }
        anyhow::ensure!(key.len() == 6, "invalid collection object key");
        let state: ObjectState = decode(&value)?;
        count = count
            .checked_add(state.offset)
            .context("document count overflow")?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Write};
    use std::process::{Child, Command, Stdio};

    use vesiro_indexer_protocol::collection::Collection;

    use super::*;
    use crate::cc::mapping::CcMappingOption;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccc";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "indexer-store-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> Store {
            Store::new(self.0.join("indexer-store"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn info(uuid: &str) -> IndexInfo {
        IndexInfo {
            uuid: uuid.into(),
            index_name: format!("index-{uuid}"),
            collection: Collection::CcWet2024,
            mapping: CcMappingOption::CcWet,
        }
    }
    fn progress(index: &sled::Db, count: u64) {
        index
            .insert(
                [0u8; 6],
                encode(&ObjectState {
                    offset: count,
                    done: true,
                })
                .unwrap(),
            )
            .unwrap();
        index.flush().unwrap();
    }
    struct Holder(
        Child,
        #[allow(dead_code)] std::io::BufReader<std::process::ChildStdout>,
    );
    impl Holder {
        fn start(store: &Store, target: &str) -> Self {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "store::tests::lock_holder", "--nocapture"])
                .env("INDEXER_TEST_ROOT", &store.root)
                .env("INDEXER_TEST_TARGET", target)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
            loop {
                let mut line = String::new();
                assert!(
                    reader.read_line(&mut line).unwrap() > 0,
                    "lock holder exited before acquiring lock"
                );
                if line.trim() == "LOCK_READY" {
                    break;
                }
            }
            Self(child, reader)
        }
    }
    impl Drop for Holder {
        fn drop(&mut self) {
            self.0.stdin.take();
            let status = self.0.wait().unwrap();
            assert!(status.success());
        }
    }

    #[test]
    fn lock_holder() {
        let Some(path) = std::env::var_os("INDEXER_TEST_ROOT") else {
            return;
        };
        let store = Store::new(path.into());
        let target = std::env::var("INDEXER_TEST_TARGET").unwrap();
        let _db = if target == "master" {
            store.master().unwrap()
        } else {
            store.open_index(&target).unwrap().1
        };
        println!("LOCK_READY");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }

    #[test]
    fn separate_processes_can_use_different_indices_and_list_busy_indices() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();
        store.create(info(B)).unwrap();
        let holder = Holder::start(&store, A);
        assert!(
            store
                .open_index(A)
                .err()
                .unwrap()
                .to_string()
                .contains("busy")
        );
        assert!(store.rekey(A, C).unwrap_err().to_string().contains("busy"));
        let (_, second) = store.open_index(B).unwrap();
        progress(&second, 17);
        drop(second);
        store.create(info(C)).unwrap();
        let rows = store.list().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, None);
        assert_eq!(rows[1].1, Some(17));
        drop(holder);
        assert!(store.open_index(A).is_ok());
    }

    #[test]
    fn master_contention_waits_until_other_process_releases_lock() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();
        let holder = Holder::start(&store, "master");
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            drop(holder);
        });
        assert!(store.open_index(A).is_ok());
        release.join().unwrap();
    }

    #[test]
    fn rekey_preserves_folder_and_progress_and_rejects_existing_uuid() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();
        store.create(info(B)).unwrap();
        let (_, index) = store.open_index(A).unwrap();
        progress(&index, 42);
        drop(index);
        let folder = Store::entry(&store.master().unwrap(), A).unwrap().folder;
        assert!(store.rekey(A, B).is_err());
        store.rekey(A, C).unwrap();
        assert!(store.open_index(A).is_err());
        assert_eq!(
            Store::entry(&store.master().unwrap(), C).unwrap().folder,
            folder
        );
        let (metadata, index) = store.open_index(C).unwrap();
        assert_eq!(metadata.uuid, C);
        assert_eq!(document_count(&index).unwrap(), 42);
        assert!(
            decode::<ObjectState>(&index.get([0u8; 6]).unwrap().unwrap())
                .unwrap()
                .done
        );
    }

    #[test]
    fn delete_removes_record_and_folder_but_refuses_a_busy_index() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();
        store.create(info(B)).unwrap();
        let folder = store.folder(Store::entry(&store.master().unwrap(), A).unwrap().folder);
        assert!(folder.exists());

        let holder = Holder::start(&store, A);
        assert!(
            store
                .index_info(A)
                .unwrap_err()
                .to_string()
                .contains("busy")
        );
        assert!(store.delete(A).unwrap_err().to_string().contains("busy"));
        drop(holder);

        assert_eq!(
            store.index_info(A).unwrap().index_name,
            format!("index-{A}")
        );
        assert_eq!(store.delete(A).unwrap().uuid, A);
        assert!(!folder.exists());
        assert!(store.open_index(A).is_err());
        assert!(store.delete(A).is_err()); // Deleting it again is not silently accepted.
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .map(|(i, _)| i.uuid.clone())
                .collect::<Vec<_>>(),
            vec![B.to_string()]
        );
        // The freed folder id is not handed out again in a way that resurrects the record.
        store.create(info(C)).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn stamps_the_layout_version_and_refuses_another() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();

        // Read the folder id and release the master before reopening either database by hand.
        let folder = {
            let master = store.master().unwrap();
            Store::entry(&master, A).unwrap().folder
        };
        for path in [store.root.join("master"), store.folder(folder)] {
            let db = sled::open(&path).unwrap();
            assert_eq!(
                db.get(DbPrefix::Version.key()).unwrap().unwrap().as_ref(),
                LAYOUT_VERSION.to_be_bytes(),
                "no header in {}",
                path.display()
            );
        }
        // The header is not mistaken for a collection object.
        assert_eq!(store.list().unwrap()[0].1, Some(0));

        {
            let master = sled::open(store.root.join("master")).unwrap();
            master
                .insert(DbPrefix::Version.key(), &(LAYOUT_VERSION + 1).to_be_bytes())
                .unwrap();
            master.flush().unwrap();
        }
        let error = store.list().unwrap_err().to_string();
        assert!(error.contains("layout version 2"), "{error}");

        {
            let master = sled::open(store.root.join("master")).unwrap();
            master.remove(DbPrefix::Version.key()).unwrap();
            master.flush().unwrap();
        }
        let error = store.list().unwrap_err().to_string();
        assert!(error.contains("no layout version header"), "{error}");
    }

    #[test]
    fn creates_the_records_directory_and_refuses_a_path_that_is_not_one() {
        let fixture = Fixture::new();

        // Missing parents and all: the first command to reach the store builds it.
        let nested = fixture.0.join("a").join("b").join("records");
        let store = Store::new(nested.clone());
        assert!(!nested.exists());
        store.initialize().unwrap();
        assert!(nested.is_dir());

        // Doing it again on the existing directory is not an error.
        store.initialize().unwrap();
        store.create(info(A)).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        let file = fixture.0.join("a-file");
        std::fs::write(&file, b"not a store").unwrap();
        let error = Store::new(file).initialize().unwrap_err().to_string();
        assert!(error.contains("not a directory"), "{error}");
    }

    #[test]
    fn refuses_duplicate_create_and_malformed_uuid() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.create(info(A)).unwrap();
        assert!(store.create(info(A)).is_err());
        assert!(store.create(info("bad")).is_err());
        assert!(store.rekey(A, "bad").is_err());
        assert!(store.rekey(B, C).is_err());
        store.rekey(A, A).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
    }
}

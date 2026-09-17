mod response;

use std::{
    fmt::Debug,
    io::{Cursor, Write},
    iter::Peekable,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use reqwest::Url;
use serde::Serialize;
use sled::Db;
use tokio::sync::Semaphore;
use tracing::instrument;

use crate::{
    cc::{self, mapping::Mapping},
    db::ObjectState,
    index_documents::response::BulkResponse,
    object_source::{CollectionObject, ObjectFetcher},
};

#[derive(Clone)]
struct IndexContext {
    client: reqwest::Client,
    node_url: Url,
    fetcher: ObjectFetcher,
    index_name: String,
    mapping: Arc<Box<dyn Mapping + Send + Sync>>,
    db: Db,
    doc_delta: i64,
}

#[derive()]
struct CollectionObjectProcessor {
    context: IndexContext,
    object: CollectionObject,
    object_state: ObjectState,
    indexed_documents: Arc<AtomicI64>,
    bulked_documents: Arc<AtomicI64>,
    bulked_documents_local: i64,
}

impl Drop for CollectionObjectProcessor {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        let bulked_documents_local = self.bulked_documents_local;
        if bulked_documents_local != 0 {
            self.bulked_documents_add(-bulked_documents_local);
            tracing::warn!(
                db_key = ?self.object.db_key,
                bulked_documents_local,
                "dropped processor, adjusted bulked_documents"
            );
        }
    }
}

#[derive(Debug)]
enum Action {
    Add,
    Flush,
    Done,
}

impl CollectionObjectProcessor {
    fn bulked_documents_add(&mut self, k: i64) -> i64 {
        self.bulked_documents_local += k;
        self.bulked_documents.fetch_add(k, Ordering::SeqCst)
    }

    #[instrument(skip(self))]
    fn step(&mut self) -> Action {
        let indexed_docs = self.indexed_documents.load(Ordering::SeqCst);
        let bulked_docs = self.bulked_documents_add(1);

        tracing::trace!(indexed_docs, bulked_docs, "stepped");

        if indexed_docs >= self.context.doc_delta {
            self.bulked_documents_add(-1);
            Action::Done
        } else if bulked_docs + indexed_docs >= self.context.doc_delta {
            self.bulked_documents_add(-1);
            Action::Flush
        } else {
            Action::Add
        }
    }

    #[instrument(skip(self))]
    async fn process(&mut self) -> anyhow::Result<()> {
        tracing::debug!(db_key = ?self.object.db_key, "current state");

        if self.object_state.done {
            tracing::info!(
                index_name = %self.context.index_name,
                db_collection_key = ?self.object.db_key,
                "indexing already done, skipping");
            Ok(())
        } else {
            tracing::info!(
                index_name = %self.context.index_name,
                db_collection_key = ?self.object.db_key,
                "starting indexing"
            );
            let data = self.download_object().await?;
            Ok(self.index_documents_from_data(data).await?)
        }
    }

    #[instrument(skip(self))]
    async fn download_object(&self) -> anyhow::Result<Vec<u8>> {
        tracing::debug!(
            db_key = ?self.object.db_key,
            s3_key = %self.object.s3_key,
            "downloading object"
        );
        let data = self.context.fetcher.download(&self.object).await?;
        tracing::info!(
            index_name = %self.context.index_name,
            db_collection_key = ?self.object.db_key,
            len = data.len(),
            "downloaded object");
        Ok(data)
    }

    #[instrument(skip(self, data))]
    async fn index_documents_from_data(&mut self, data: Vec<u8>) -> anyhow::Result<()> {
        use std::io::Read;

        let mut input = flate2::read::MultiGzDecoder::new(data.as_slice());
        let mut buffer = Vec::new();
        input.read_to_end(&mut buffer)?;
        let input = buffer.as_slice();

        tracing::debug!(input_len = input.len(), "decompressed object data");

        let mut objects = cc::iter::CcObjectIterator::new(input)?
            .skip(self.object_state.offset as usize)
            .peekable();

        tracing::debug!(db_key = ?self.object.db_key, ?self.object_state.offset, "created object iterator");
        let buffer = Box::<[u8; 5 * 1024 * 1024]>::new_uninit();
        let mut buffer = unsafe { buffer.assume_init() };
        loop {
            if objects.peek().is_none() {
                break;
            } else {
                if !self.bulk(&mut objects, buffer.as_mut_slice()).await? {
                    // Bulk returning false indicates that it received the Done signal.
                    tracing::debug!("drop bulk");
                    break;
                }
            }
        }
        tracing::info!(
            index_name = %self.context.index_name,
            db_collection_key = ?self.object.db_key,
            "finished processing collection object"
        );
        Ok(())
    }

    /// Returns true if the bulk was flushed.
    #[instrument(skip(self, documents, buffer))]
    async fn bulk<T: Serialize>(
        &mut self,
        documents: &mut Peekable<impl Iterator<Item = anyhow::Result<T>>>,
        buffer: &mut [u8],
    ) -> anyhow::Result<bool> {
        let mut cursor = Cursor::new(buffer);
        let mut n = 0;

        loop {
            let step = self.step();
            tracing::trace!(?step, "received step");

            match step {
                Action::Add => match documents.peek() {
                    Some(Ok(document)) => {
                        let (id, document) = match self
                            .context
                            .mapping
                            .transform_document(&serde_json::to_value(document)?)
                        {
                            Ok(res) => res,
                            Err(err) => {
                                tracing::warn!(?err, "failed to transform document");
                                documents.next().transpose()?;
                                self.bulked_documents_add(-1);
                                continue;
                            }
                        };
                        if self.add_bulk_document(&id, &document, &mut cursor)? {
                            tracing::trace!(?id, "added document to bulk");
                            documents.next().transpose()?;
                            n += 1;
                        } else {
                            self.bulked_documents_add(-1);
                            let pos = cursor.position() as usize;
                            self.flush_bulk(&cursor.get_ref()[..pos], n).await?;
                            self.object_state.offset += n as u64;
                            self.update_object_state()?;
                            return Ok(true);
                        }
                    }
                    Some(Err(_)) => {
                        self.bulked_documents_add(-1);
                        // Throw the error from the iterator.
                        documents.next().transpose()?;
                    }
                    None => {
                        self.bulked_documents_add(-1);
                        let pos = cursor.position() as usize;
                        self.flush_bulk(&cursor.get_ref()[..pos], n).await?;
                        self.object_state.offset += n as u64;
                        self.object_state.done = true;
                        self.update_object_state()?;
                        return Ok(true);
                    }
                },
                Action::Flush => {
                    let pos = cursor.position() as usize;
                    self.flush_bulk(&cursor.get_ref()[..pos], n).await?;
                    self.object_state.offset += n as u64;
                    self.update_object_state()?;
                    return Ok(true);
                }
                Action::Done => return Ok(false),
            }
        }
    }

    fn update_object_state(&self) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(&self.object_state, bincode::config::standard())?;
        self.context.db.insert(&self.object.db_key, encoded)?;
        Ok(())
    }

    fn add_bulk_document<T: Serialize>(
        &self,
        id: &String,
        document: &T,
        cursor: &mut Cursor<&mut [u8]>,
    ) -> anyhow::Result<bool> {
        let mut buffer = Vec::new();
        let mut tmp_cursor = Cursor::new(&mut buffer);

        let cmd = serde_json::json!({"create": {
            "_index": self.context.index_name,
            "_id": id
        }});

        serde_json::to_writer(tmp_cursor.by_ref(), &cmd)?;
        tmp_cursor.write_all(b"\n")?;
        serde_json::to_writer(tmp_cursor.by_ref(), document)?;
        tmp_cursor.write_all(b"\n")?;

        if (tmp_cursor.position() as usize) + cursor.position() as usize > cursor.get_ref().len() {
            Ok(false)
        } else {
            cursor.write_all(&buffer)?;
            Ok(true)
        }
    }

    #[instrument(skip(self, body))]
    async fn flush_bulk(&mut self, body: &[u8], n: i64) -> anyhow::Result<()> {
        tracing::debug!(
            buffer_len = body.len(),
            ?self.indexed_documents,
            ?self.bulked_documents,
            "flushing bulk request"
        );

        if body.is_empty() {
            assert!(n == 0);
            tracing::debug!("bulk body is empty, skipping flush");

            // Not sleeping here can cause starvation of other tasks that has pending bulks. The
            // required sleep duration could possibly be lower.
            tokio::time::sleep(Duration::from_millis(100)).await;

            return Ok(());
        }

        let response = self
            .context
            .client
            .post(self.context.node_url.join("_bulk")?)
            .header("Content-Type", "application/x-ndjson")
            .body(body.to_vec())
            .send()
            .await?;
        tracing::debug!(?response, "bulk response");
        /*
        let body = response.text().await?;
        tracing::debug!(?body, "bulk response body");
        let body: BulkResponse = serde_json::from_str(&body)?;
        */
        let body: BulkResponse = response.json().await?;

        // TODO:
        //   The slight lag between increasing and decreasing the counters is not optimal. There
        //   will be moments when the sum of indexed_documents and bulked_documents is greater than
        //   doc_delta because of documents being counted twice. This can trigger premature flushes.
        //
        //   Using locks is not really a solution I like. Perhaps it would be possible to increment
        //   and decrement in one instruction if both indexed_documents and bulked_documents were
        //   the same i64 sharing 32 bits each.
        if body.errors {
            let mut already_exists = 0;
            let mut created = 0;
            let mut failed = 0;
            for item in body.items {
                if let Some(err) = item.create.error {
                    if item.create.status == 409
                        && err.get("type").and_then(serde_json::Value::as_str)
                            == Some("version_conflict_engine_exception")
                    {
                        already_exists += 1;
                        tracing::debug!(id = %item.create.id, "document already exists");
                    } else {
                        failed += 1;
                        // TODO:
                        //   Documents that failed to be indexed should be retried in some cases.
                        tracing::error!(?err, "bulk item error");
                    }
                } else {
                    created += 1;
                    self.indexed_documents.fetch_add(1, Ordering::SeqCst);
                }
                self.bulked_documents_add(-1);
            }
            if already_exists > 0 {
                tracing::info!(created, already_exists, failed, "bulk response summary");
            }
        } else {
            self.indexed_documents.fetch_add(n, Ordering::SeqCst);
            self.bulked_documents_add(-n);
        }

        tracing::debug!(?self.indexed_documents, ?self.bulked_documents, "flushed bulk request");
        Ok(())
    }
}

#[instrument(skip(db, client, fetcher, objects, mapping))]
pub async fn spawn_collection_object_processors(
    db: sled::Db,
    client: reqwest::Client,
    fetcher: ObjectFetcher,
    node_url: Url,
    index_name: String,
    mapping: Box<dyn Mapping + Send + Sync>,
    doc_delta: i64,
    objects: Vec<CollectionObject>,
    worker_amount: usize,
) -> anyhow::Result<()> {
    if doc_delta < 0 {
        todo!("negative doc delta is not supported yet");
    }

    let semaphore = Arc::new(Semaphore::new(worker_amount));
    let context = IndexContext {
        client,
        node_url,
        fetcher,
        index_name,
        mapping: Arc::new(mapping),
        db,
        doc_delta,
    };

    let indexed_documents = Arc::new(AtomicI64::new(0));
    let bulked_documents = Arc::new(AtomicI64::new(0));
    let mut tasks = vec![];

    for object in objects {
        if indexed_documents.load(Ordering::SeqCst) >= doc_delta {
            tracing::info!("indexing limit reached, stop spawning more processors");
            break;
        }
        let permit = semaphore.clone().acquire_owned().await?;

        let object_state = if let Some(ivec) = context.db.get(&object.db_key)? {
            bincode::decode_from_slice(&ivec, bincode::config::standard())?.0
        } else {
            ObjectState::default()
        };

        let mut processor = CollectionObjectProcessor {
            context: context.clone(),
            object,
            object_state,
            indexed_documents: indexed_documents.clone(),
            bulked_documents: bulked_documents.clone(),
            bulked_documents_local: 0,
        };
        tasks.push(tokio::spawn(async move {
            tracing::debug!(db_key = ?processor.object.db_key, "starting processing collection object");
            let _permit = permit;
            if let Err(e) = processor.process().await {
                tracing::error!(?e, "error processing object");
            }
        }));
    }

    for task in tasks {
        task.await?;
    }

    Ok(())
}

//! Opt-in HTTP Range VFS used only by the retained GBZ-base comparison.

use gbz::FullPathName;
use gbz_base::{HaplotypeOutput, Subgraph, SubgraphQuery};
use reqwest::blocking::Client;
use reqwest::header::{
    ACCEPT_ENCODING, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE,
    ETAG, IF_RANGE, RANGE,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_vfs::{DatabaseHandle, LockKind, OpenAccess, OpenKind, OpenOptions, Vfs, WalDisabled};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT_VFS_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct HttpSqliteQuery<'a> {
    pub url: &'a str,
    pub sample: &'a str,
    pub contig: &'a str,
    pub start: u64,
    pub end: u64,
    pub context: u64,
    pub chunk_bytes: usize,
    pub cache_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSqliteQueryReport {
    pub url: String,
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
    pub context: u64,
    pub object_bytes: u64,
    pub chunk_bytes: usize,
    pub cache_capacity_bytes: usize,
    pub open_and_query_wall_ms: f64,
    pub query_wall_ms: f64,
    pub request_count: u64,
    pub head_request_count: u64,
    pub range_request_count: u64,
    pub response_bytes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_peak_bytes: u64,
    pub etag: Option<String>,
    pub output_nodes: usize,
    pub output_json_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Workload {
    archive_sha256: String,
    queries: Vec<WorkloadQuery>,
}

#[derive(Debug, Deserialize)]
struct WorkloadQuery {
    id: String,
    class: String,
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    context: u64,
}

#[derive(Debug, Default)]
struct Metrics {
    request_count: u64,
    head_request_count: u64,
    range_request_count: u64,
    response_bytes: u64,
    cache_hits: u64,
    cache_misses: u64,
    cache_peak_bytes: u64,
}

#[derive(Debug)]
struct Shared {
    metrics: Metrics,
    object_bytes: u64,
    etag: Option<String>,
}

#[derive(Clone)]
struct HttpRangeVfs {
    client: Client,
    chunk_bytes: usize,
    cache_bytes: usize,
    shared: Arc<Mutex<Shared>>,
}

struct HttpRangeHandle {
    url: String,
    client: Client,
    chunk_bytes: usize,
    cache_bytes: usize,
    cache_size: usize,
    cache: BTreeMap<u64, Vec<u8>>,
    cache_order: VecDeque<u64>,
    shared: Arc<Mutex<Shared>>,
    lock: LockKind,
}

fn io_error(kind: io::ErrorKind, message: impl Into<String>) -> io::Error {
    io::Error::new(kind, message.into())
}

impl HttpRangeVfs {
    fn fetch_metadata(&self, url: &str) -> io::Result<(u64, Option<String>)> {
        let response = self
            .client
            .head(url)
            .header(CACHE_CONTROL, "no-store")
            .send()
            .map_err(|error| io_error(io::ErrorKind::Other, error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(io_error(
                io::ErrorKind::Other,
                format!("HEAD {url} returned {status}"),
            ));
        }
        let accepts_ranges = response
            .headers()
            .get(ACCEPT_RANGES)
            .and_then(|value| value.to_str().ok());
        if accepts_ranges != Some("bytes") {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                format!("HEAD {url} did not advertise Accept-Ranges: bytes"),
            ));
        }
        let length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| io_error(io::ErrorKind::InvalidData, "missing Content-Length"))?;
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                io_error(
                    io::ErrorKind::InvalidData,
                    "missing ETag for immutable HTTP SQLite object",
                )
            })?;
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?;
        shared.metrics.request_count += 1;
        shared.metrics.head_request_count += 1;
        shared.object_bytes = length;
        shared.etag = Some(etag.clone());
        Ok((length, Some(etag)))
    }
}

impl Vfs for HttpRangeVfs {
    type Handle = HttpRangeHandle;

    fn open(&self, db: &str, opts: OpenOptions) -> io::Result<Self::Handle> {
        if opts.kind != OpenKind::MainDb || opts.access != OpenAccess::Read {
            return Err(io_error(
                io::ErrorKind::PermissionDenied,
                "HTTP SQLite benchmark VFS is immutable and read-only",
            ));
        }
        self.fetch_metadata(db)?;
        Ok(HttpRangeHandle {
            url: db.to_owned(),
            client: self.client.clone(),
            chunk_bytes: self.chunk_bytes,
            cache_bytes: self.cache_bytes,
            cache_size: 0,
            cache: BTreeMap::new(),
            cache_order: VecDeque::new(),
            shared: Arc::clone(&self.shared),
            lock: LockKind::None,
        })
    }

    fn delete(&self, _db: &str) -> io::Result<()> {
        Err(io_error(io::ErrorKind::PermissionDenied, "read-only VFS"))
    }

    fn exists(&self, db: &str) -> io::Result<bool> {
        if db.ends_with("-journal") || db.ends_with("-wal") || db.ends_with("-shm") {
            return Ok(false);
        }
        self.fetch_metadata(db).map(|_| true)
    }

    fn temporary_name(&self) -> String {
        "pangenome-range-http-vfs-temporary-disabled".to_owned()
    }

    fn random(&self, buffer: &mut [i8]) {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes();
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = i8::from_le_bytes([seed[index % seed.len()]]);
        }
    }

    fn sleep(&self, duration: Duration) -> Duration {
        std::thread::sleep(duration);
        duration
    }
}

impl HttpRangeHandle {
    fn download_chunk(
        &self,
        chunk_offset: u64,
        end: u64,
        object_bytes: u64,
        etag: Option<&str>,
    ) -> io::Result<Vec<u8>> {
        let mut request = self
            .client
            .get(&self.url)
            .header(RANGE, format!("bytes={chunk_offset}-{end}"))
            .header(ACCEPT_ENCODING, "identity")
            .header(CACHE_CONTROL, "no-store");
        if let Some(etag) = etag {
            request = request.header(IF_RANGE, etag);
        }
        let response = request
            .send()
            .map_err(|error| io_error(io::ErrorKind::Other, error.to_string()))?;
        if response.status().as_u16() != 206 {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                format!("range GET returned {} instead of 206", response.status()),
            ));
        }
        let expected_content_range = format!("bytes {chunk_offset}-{end}/{object_bytes}");
        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok());
        if content_range != Some(expected_content_range.as_str()) {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                format!(
                    "invalid Content-Range {content_range:?}; expected {expected_content_range}"
                ),
            ));
        }
        let content_encoding = response
            .headers()
            .get(CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok());
        if content_encoding.is_some_and(|value| value != "identity") {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                format!("range GET returned transformed content encoding {content_encoding:?}"),
            ));
        }
        let response_etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok());
        if etag.is_some_and(|expected| response_etag != Some(expected)) {
            return Err(io_error(
                io::ErrorKind::InvalidData,
                "ETag changed during query",
            ));
        }
        let bytes = response
            .bytes()
            .map_err(|error| io_error(io::ErrorKind::Other, error.to_string()))?
            .to_vec();
        let expected_len = usize::try_from(end - chunk_offset + 1)
            .map_err(|_| io_error(io::ErrorKind::InvalidData, "range length exceeds usize"))?;
        if bytes.len() != expected_len {
            return Err(io_error(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "range returned {} bytes; expected {expected_len}",
                    bytes.len()
                ),
            ));
        }
        Ok(bytes)
    }

    fn fetch_chunk(&mut self, chunk_offset: u64) -> io::Result<Vec<u8>> {
        if let Some(bytes) = self.cache.get(&chunk_offset) {
            self.shared
                .lock()
                .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?
                .metrics
                .cache_hits += 1;
            return Ok(bytes.clone());
        }

        let (object_bytes, etag) = {
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?;
            shared.metrics.cache_misses += 1;
            (shared.object_bytes, shared.etag.clone())
        };
        let chunk_len = u64::try_from(self.chunk_bytes)
            .map_err(|_| io_error(io::ErrorKind::InvalidInput, "chunk size exceeds u64"))?;
        let end = chunk_offset
            .saturating_add(chunk_len)
            .min(object_bytes)
            .saturating_sub(1);
        if chunk_offset >= object_bytes {
            return Err(io_error(
                io::ErrorKind::UnexpectedEof,
                "read past object end",
            ));
        }
        let bytes = self.download_chunk(chunk_offset, end, object_bytes, etag.as_deref())?;

        while self.cache_size.saturating_add(bytes.len()) > self.cache_bytes {
            let Some(evicted_offset) = self.cache_order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.cache.remove(&evicted_offset) {
                self.cache_size = self.cache_size.saturating_sub(evicted.len());
            }
        }
        if bytes.len() <= self.cache_bytes {
            self.cache_size += bytes.len();
            self.cache.insert(chunk_offset, bytes.clone());
            self.cache_order.push_back(chunk_offset);
        }

        let mut shared = self
            .shared
            .lock()
            .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?;
        shared.metrics.request_count += 1;
        shared.metrics.range_request_count += 1;
        shared.metrics.response_bytes += u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        shared.metrics.cache_peak_bytes = shared
            .metrics
            .cache_peak_bytes
            .max(u64::try_from(self.cache_size).unwrap_or(u64::MAX));
        Ok(bytes)
    }
}

impl DatabaseHandle for HttpRangeHandle {
    type WalIndex = WalDisabled;

    fn size(&self) -> io::Result<u64> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?
            .object_bytes)
    }

    fn read_exact_at(&mut self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let chunk_bytes = u64::try_from(self.chunk_bytes)
            .map_err(|_| io_error(io::ErrorKind::InvalidInput, "chunk size exceeds u64"))?;
        let mut copied = 0_usize;
        while copied < buf.len() {
            let absolute = offset
                .checked_add(u64::try_from(copied).map_err(|_| {
                    io_error(io::ErrorKind::InvalidInput, "read offset exceeds u64")
                })?)
                .ok_or_else(|| io_error(io::ErrorKind::InvalidInput, "read offset overflow"))?;
            let chunk_offset = absolute / chunk_bytes * chunk_bytes;
            let chunk = self.fetch_chunk(chunk_offset)?;
            let within = usize::try_from(absolute - chunk_offset)
                .map_err(|_| io_error(io::ErrorKind::InvalidInput, "chunk offset exceeds usize"))?;
            let available = chunk.len().saturating_sub(within);
            let take = available.min(buf.len() - copied);
            if take == 0 {
                return Err(io_error(io::ErrorKind::UnexpectedEof, "short range read"));
            }
            buf[copied..copied + take].copy_from_slice(&chunk[within..within + take]);
            copied += take;
        }
        Ok(())
    }

    fn write_all_at(&mut self, _buf: &[u8], _offset: u64) -> io::Result<()> {
        Err(io_error(io::ErrorKind::PermissionDenied, "read-only VFS"))
    }

    fn sync(&mut self, _data_only: bool) -> io::Result<()> {
        Ok(())
    }

    fn set_len(&mut self, _size: u64) -> io::Result<()> {
        Err(io_error(io::ErrorKind::PermissionDenied, "read-only VFS"))
    }

    fn lock(&mut self, lock: LockKind) -> io::Result<bool> {
        if matches!(lock, LockKind::None | LockKind::Shared) {
            self.lock = lock;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn reserved(&mut self) -> io::Result<bool> {
        Ok(false)
    }

    fn current_lock(&self) -> io::Result<LockKind> {
        Ok(self.lock)
    }

    fn wal_index(&self, _readonly: bool) -> io::Result<Self::WalIndex> {
        Err(io_error(io::ErrorKind::Unsupported, "WAL is disabled"))
    }
}

/// Runs the unchanged upstream GBZ-base query through a strict HTTP Range VFS.
///
/// # Errors
///
/// Returns an error when the origin violates the immutable range contract, the
/// `SQLite` database cannot be opened, or the upstream query fails.
pub fn run_http_sqlite_query(
    query: &HttpSqliteQuery<'_>,
) -> Result<HttpSqliteQueryReport, Box<dyn std::error::Error + Send + Sync>> {
    if query.chunk_bytes == 0 || !query.chunk_bytes.is_power_of_two() {
        return Err("HTTP SQLite chunk size must be a non-zero power of two".into());
    }
    if query.cache_bytes < query.chunk_bytes {
        return Err("HTTP SQLite cache must hold at least one chunk".into());
    }
    let client = Client::builder().build()?;
    let shared = Arc::new(Mutex::new(Shared {
        metrics: Metrics::default(),
        object_bytes: 0,
        etag: None,
    }));
    let vfs_name = format!(
        "pangenome-range-http-{}",
        NEXT_VFS_ID.fetch_add(1, Ordering::Relaxed)
    );
    sqlite_vfs::register(
        &vfs_name,
        HttpRangeVfs {
            client,
            chunk_bytes: query.chunk_bytes,
            cache_bytes: query.cache_bytes,
            shared: Arc::clone(&shared),
        },
        true,
    )?;

    let total_started = Instant::now();
    let database = gbz_base::GBZBase::open(query.url)?;
    let mut graph = gbz_base::GraphInterface::new(&database)?;
    let path = FullPathName::reference(query.sample, query.contig);
    let interval_start = usize::try_from(query.start)?;
    let interval_end = usize::try_from(query.end)?;
    let context = usize::try_from(query.context)?;
    let subgraph_query = SubgraphQuery::path_interval(&path, interval_start..interval_end)
        .with_context(context)
        .with_haplotypes(HaplotypeOutput::All);
    let query_started = Instant::now();
    let mut subgraph = Subgraph::new();
    subgraph.from_db(&mut graph, &subgraph_query)?;
    let query_wall_ms = query_started.elapsed().as_secs_f64() * 1_000.0;
    let output_nodes = subgraph.nodes();
    let mut output = Vec::new();
    subgraph.write_json(&mut output, false)?;
    let output_json_sha256 = format!("{:x}", Sha256::digest(&output));
    let open_and_query_wall_ms = total_started.elapsed().as_secs_f64() * 1_000.0;
    drop(graph);
    drop(database);

    let shared = shared
        .lock()
        .map_err(|_| io_error(io::ErrorKind::Other, "HTTP VFS metrics lock poisoned"))?;
    Ok(HttpSqliteQueryReport {
        url: query.url.to_owned(),
        sample: query.sample.to_owned(),
        contig: query.contig.to_owned(),
        start: query.start,
        end: query.end,
        context: query.context,
        object_bytes: shared.object_bytes,
        chunk_bytes: query.chunk_bytes,
        cache_capacity_bytes: query.cache_bytes,
        open_and_query_wall_ms,
        query_wall_ms,
        request_count: shared.metrics.request_count,
        head_request_count: shared.metrics.head_request_count,
        range_request_count: shared.metrics.range_request_count,
        response_bytes: shared.metrics.response_bytes,
        cache_hits: shared.metrics.cache_hits,
        cache_misses: shared.metrics.cache_misses,
        cache_peak_bytes: shared.metrics.cache_peak_bytes,
        etag: shared.etag.clone(),
        output_nodes,
        output_json_sha256,
    })
}

/// Runs each workload query with a fresh HTTP VFS and validates its serialized
/// GBZ-base result against the retained source-oracle hashes.
///
/// # Errors
///
/// Returns an error if an input report is malformed, a query fails, an output
/// hash differs from the source oracle, or the report cannot be written.
pub fn run_http_sqlite_workload(
    url: &str,
    workload_path: &Path,
    oracle_summary_path: &Path,
    output_path: &Path,
    chunk_bytes: usize,
    cache_bytes: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let workload_bytes = std::fs::read(workload_path)?;
    let workload: Workload = serde_json::from_slice(&workload_bytes)?;
    let oracle: serde_json::Value = serde_json::from_slice(&std::fs::read(oracle_summary_path)?)?;
    let oracle_rows = oracle["queries"]
        .as_array()
        .ok_or("oracle summary does not contain a queries array")?;
    let mut expected_hashes = BTreeMap::new();
    for row in oracle_rows {
        let Some(query_id) = row["query_id"].as_str() else {
            continue;
        };
        let Some(source_hash) = row["source_json_sha256"].as_str() else {
            continue;
        };
        if let Some(previous) = expected_hashes.insert(query_id.to_owned(), source_hash.to_owned())
        {
            if previous != source_hash {
                return Err(format!("oracle hash changed between passes for {query_id}").into());
            }
        }
    }

    let mut rows = Vec::with_capacity(workload.queries.len());
    let mut all_correct = true;
    for (index, item) in workload.queries.iter().enumerate() {
        eprintln!(
            "HTTP SQLite cold query {}/{}: {}",
            index + 1,
            workload.queries.len(),
            item.id
        );
        let expected = expected_hashes
            .get(&item.id)
            .ok_or_else(|| format!("missing source-oracle hash for {}", item.id))?;
        let report = run_http_sqlite_query(&HttpSqliteQuery {
            url,
            sample: &item.sample,
            contig: &item.contig,
            start: item.start,
            end: item.end,
            context: item.context,
            chunk_bytes,
            cache_bytes,
        })?;
        let correctness = report.output_json_sha256 == *expected;
        all_correct &= correctness;
        rows.push(serde_json::json!({
            "queryId": item.id,
            "queryClass": item.class,
            "expectedSourceJsonSha256": expected,
            "correctness": correctness,
            "measurement": report,
        }));
    }

    let output = serde_json::json!({
        "schemaVersion": 1,
        "databaseUrl": url,
        "workloadPath": workload_path,
        "workloadSha256": format!("{:x}", Sha256::digest(&workload_bytes)),
        "archiveSha256": workload.archive_sha256,
        "queryCount": workload.queries.len(),
        "cacheScope": "cold: each query opens a fresh SQLite connection, HTTP client, and byte-bounded VFS cache",
        "httpPolicy": {
            "chunkBytes": chunk_bytes,
            "cacheBytes": cache_bytes,
            "requestCacheControl": "no-store",
            "rangeValidation": "strict 206, Content-Range, response length, and stable ETag",
        },
        "semantics": "unchanged GBZ-base Subgraph::from_db path-interval query with all haplotypes and no snarls",
        "allCorrect": all_correct,
        "queries": rows,
    });
    let mut encoded = serde_json::to_vec_pretty(&output)?;
    encoded.push(b'\n');
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, encoded)?;
    if !all_correct {
        return Err("HTTP SQLite workload failed source-oracle equality".into());
    }
    Ok(())
}

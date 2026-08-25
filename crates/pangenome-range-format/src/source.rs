use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// A storage-independent, random-access source of bytes.
///
/// Offsets are always 64-bit. Implementations must return exactly `length`
/// bytes or an error. A zero-length read is valid at any offset up to `len()`.
pub trait RangeSource: Send + Sync {
    /// Returns the source length in bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing source cannot determine its length.
    fn len(&self) -> io::Result<u64>;

    /// Returns whether the source contains no bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if [`RangeSource::len`] fails.
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Reads exactly `length` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is invalid, unavailable, or cannot be read.
    fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>>;
}

/// A local-file implementation of [`RangeSource`].
#[derive(Debug)]
pub struct FileRangeSource {
    file: File,
    len: u64,
}

impl FileRangeSource {
    /// Opens a local file and snapshots its current length.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or its metadata read.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }

    fn validate(&self, offset: u64, length: usize) -> io::Result<()> {
        let length = u64::try_from(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "range length does not fit in u64",
            )
        })?;
        let end = offset.checked_add(length).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "range end overflows u64")
        })?;
        if end > self.len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("range {offset}..{end} exceeds source length {}", self.len),
            ));
        }
        Ok(())
    }
}

impl RangeSource for FileRangeSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.len)
    }

    fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        self.validate(offset, length)?;
        let mut bytes = vec![0_u8; length];
        read_exact_at(&self.file, &mut bytes, offset)?;
        Ok(bytes)
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;

    while !buffer.is_empty() {
        let count = file.read_at(buffer, offset)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file changed while a range was being read",
            ));
        }
        offset = offset
            .checked_add(u64::try_from(count).expect("usize fits in u64 on supported Unix targets"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "offset overflow"))?;
        buffer = &mut buffer[count..];
    }
    Ok(())
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;

    while !buffer.is_empty() {
        let count = file.seek_read(buffer, offset)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file changed while a range was being read",
            ));
        }
        offset = offset
            .checked_add(u64::try_from(count).expect("usize fits in u64 on Windows targets"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "offset overflow"))?;
        buffer = &mut buffer[count..];
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
compile_error!("FileRangeSource currently supports Unix and Windows targets");

/// One attempted range read, in call order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeRead {
    pub sequence: u64,
    pub offset: u64,
    pub length: usize,
    pub succeeded: bool,
}

/// Aggregate statistics for a trace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceSummary {
    pub reads: Vec<RangeRead>,
    pub read_operations: u64,
    pub successful_operations: u64,
    pub total_bytes_requested: u64,
    pub unique_bytes_requested: u64,
    pub duplicate_bytes_requested: u64,
    /// Reads that could be combined with another read because they overlap or touch.
    pub mergeable_reads: u64,
    /// Number of disjoint ranges after overlapping and adjacent requests are coalesced.
    pub coalesced_ranges: u64,
    pub largest_read: Option<usize>,
    pub smallest_read: Option<usize>,
}

/// A [`RangeSource`] decorator that records every attempted range read.
#[derive(Debug)]
pub struct TracingRangeSource<T> {
    inner: T,
    next_sequence: AtomicU64,
    reads: Mutex<Vec<RangeRead>>,
}

impl<T> TracingRangeSource<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            next_sequence: AtomicU64::new(0),
            reads: Mutex::new(Vec::new()),
        }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Clears all recorded reads. Sequence numbers remain monotonic.
    pub fn clear(&self) {
        self.reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Returns an immutable snapshot and derived metrics.
    pub fn summary(&self) -> TraceSummary {
        let mut reads = self
            .reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        reads.sort_by_key(|read| read.sequence);

        let read_operations = u64::try_from(reads.len()).unwrap_or(u64::MAX);
        let successful_operations =
            u64::try_from(reads.iter().filter(|read| read.succeeded).count()).unwrap_or(u64::MAX);
        let total_bytes_requested = reads.iter().fold(0_u64, |total, read| {
            total.saturating_add(u64::try_from(read.length).unwrap_or(u64::MAX))
        });
        let largest_read = reads.iter().map(|read| read.length).max();
        let smallest_read = reads.iter().map(|read| read.length).min();

        let mut ranges: Vec<(u64, u64)> = reads
            .iter()
            .filter_map(|read| {
                let length = u64::try_from(read.length).ok()?;
                let end = read.offset.checked_add(length)?;
                (end > read.offset).then_some((read.offset, end))
            })
            .collect();
        ranges.sort_unstable();

        let mut unique_bytes_requested = 0_u64;
        let mut coalesced_ranges = 0_u64;
        let mut current: Option<(u64, u64)> = None;
        for (start, end) in ranges {
            match current {
                None => {
                    current = Some((start, end));
                    coalesced_ranges += 1;
                }
                Some((current_start, current_end)) if start <= current_end => {
                    current = Some((current_start, current_end.max(end)));
                }
                Some((current_start, current_end)) => {
                    unique_bytes_requested = unique_bytes_requested
                        .saturating_add(current_end.saturating_sub(current_start));
                    current = Some((start, end));
                    coalesced_ranges += 1;
                }
            }
        }
        if let Some((start, end)) = current {
            unique_bytes_requested =
                unique_bytes_requested.saturating_add(end.saturating_sub(start));
        }
        let non_empty_reads =
            u64::try_from(reads.iter().filter(|read| read.length > 0).count()).unwrap_or(u64::MAX);

        TraceSummary {
            reads,
            read_operations,
            successful_operations,
            total_bytes_requested,
            unique_bytes_requested,
            duplicate_bytes_requested: total_bytes_requested.saturating_sub(unique_bytes_requested),
            mergeable_reads: non_empty_reads.saturating_sub(coalesced_ranges),
            coalesced_ranges,
            largest_read,
            smallest_read,
        }
    }
}

impl<T: RangeSource> RangeSource for TracingRangeSource<T> {
    fn len(&self) -> io::Result<u64> {
        self.inner.len()
    }

    fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let result = self.inner.read_range(offset, length);
        let read = RangeRead {
            sequence,
            offset,
            length,
            succeeded: result.is_ok(),
        };
        self.reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(read);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct BytesSource(Vec<u8>);

    impl RangeSource for BytesSource {
        fn len(&self) -> io::Result<u64> {
            Ok(u64::try_from(self.0.len()).expect("test source length fits in u64"))
        }

        fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
            let start = usize::try_from(offset).map_err(io::Error::other)?;
            let end = start
                .checked_add(length)
                .filter(|end| *end <= self.0.len())
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            Ok(self.0[start..end].to_vec())
        }
    }

    #[test]
    fn file_source_reads_positioned_ranges() {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pangenome-range-source-{}-{id}",
            std::process::id()
        ));
        std::fs::write(&path, b"abcdefghij").unwrap();

        let source = FileRangeSource::open(&path).unwrap();
        assert_eq!(source.len().unwrap(), 10);
        assert_eq!(source.read_range(3, 4).unwrap(), b"defg");
        assert_eq!(source.read_range(0, 2).unwrap(), b"ab");
        assert_eq!(source.read_range(10, 0).unwrap(), b"");
        assert_eq!(
            source.read_range(9, 2).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn trace_accounts_for_overlap_adjacency_and_failures() {
        let source = TracingRangeSource::new(BytesSource((0_u8..20).collect()));
        source.read_range(0, 5).unwrap();
        source.read_range(3, 4).unwrap();
        source.read_range(7, 2).unwrap();
        source.read_range(12, 3).unwrap();
        assert!(source.read_range(19, 2).is_err());

        let summary = source.summary();
        assert_eq!(summary.read_operations, 5);
        assert_eq!(summary.successful_operations, 4);
        assert_eq!(summary.total_bytes_requested, 16);
        assert_eq!(summary.unique_bytes_requested, 14);
        assert_eq!(summary.duplicate_bytes_requested, 2);
        assert_eq!(summary.coalesced_ranges, 3);
        assert_eq!(summary.mergeable_reads, 2);
        assert_eq!(summary.largest_read, Some(5));
        assert_eq!(summary.smallest_read, Some(2));
        assert!(!summary.reads[4].succeeded);
    }

    #[test]
    fn clear_removes_trace() {
        let source = TracingRangeSource::new(BytesSource(vec![0; 4]));
        source.read_range(0, 1).unwrap();
        source.clear();
        assert_eq!(source.summary(), TraceSummary::default());
    }
}

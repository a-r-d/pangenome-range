use crate::CatalogPath;
use gbz::support::{self, Orientation};
use gbz::{GBWT, GENERIC_HAPLOTYPE, GENERIC_SAMPLE, REFERENCE_SAMPLES_KEY};
use pangenome_range_format::{FileRangeSource, RangeSource};
use serde::Serialize;
use serde_json::{Value, json};
use simple_sds::serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const CATALOG_MAGIC: &[u8; 8] = b"PMPC0001";
const PAGE_MAGIC: &[u8; 8] = b"PMCPAGE1";
const CATALOG_VERSION: u32 = 1;
const HEADER_BYTES: usize = 64;
const DIRECTORY_ENTRY_BYTES: usize = 48;
const CODEC_NONE: u8 = 0;
const CODEC_ZSTD_3: u8 = 1;
const MAX_RECORDS_PER_PAGE: usize = 65_536;
const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn required<'a>(values: &'a HashMap<String, String>, key: &str) -> io::Result<&'a str> {
    values
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing required argument {key}")))
}

fn parse_usize(values: &HashMap<String, String>, key: &str) -> io::Result<usize> {
    required(values, key)?
        .parse()
        .map_err(|_| invalid(format!("invalid unsigned integer for {key}")))
}

fn usize_from_u64(value: u64, field: &str) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid(format!("{field} does not fit in usize")))
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn get_u32(input: &[u8], offset: usize) -> io::Result<u32> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("truncated u32"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn get_u64(input: &[u8], offset: usize) -> io::Result<u64> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or_else(|| invalid("truncated u64"))?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn put_varint(output: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        output.push(u8::try_from(value & 0x7f).expect("seven bits fit in u8") | 0x80);
        value >>= 7;
    }
    output.push(u8::try_from(value).expect("final varint byte fits in u8"));
}

fn get_varint(input: &[u8], cursor: &mut usize) -> io::Result<u64> {
    let mut value = 0_u64;
    for shift in (0..=63).step_by(7) {
        let byte = *input
            .get(*cursor)
            .ok_or_else(|| invalid("truncated varint"))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(invalid("varint overflow"));
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(invalid("varint overflow"))
}

fn common_prefix(left: &[u8], right: &[u8]) -> usize {
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

fn encode_string(output: &mut Vec<u8>, previous: &str, value: &str) {
    let mut prefix = common_prefix(previous.as_bytes(), value.as_bytes());
    while prefix > 0 && !previous.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let suffix = &value.as_bytes()[prefix..];
    put_varint(output, prefix as u64);
    put_varint(output, suffix.len() as u64);
    output.extend_from_slice(suffix);
}

fn decode_string(input: &[u8], cursor: &mut usize, previous: &str) -> io::Result<String> {
    let prefix = usize_from_u64(get_varint(input, cursor)?, "string prefix")?;
    let suffix_len = usize_from_u64(get_varint(input, cursor)?, "string suffix length")?;
    if prefix > previous.len() || !previous.is_char_boundary(prefix) {
        return Err(invalid("invalid front-coded string prefix"));
    }
    let suffix_end = cursor
        .checked_add(suffix_len)
        .ok_or_else(|| invalid("string suffix overflow"))?;
    let suffix = input
        .get(*cursor..suffix_end)
        .ok_or_else(|| invalid("truncated string suffix"))?;
    let suffix = std::str::from_utf8(suffix).map_err(|_| invalid("invalid UTF-8 string"))?;
    *cursor = suffix_end;
    let mut result = previous[..prefix].to_owned();
    result.push_str(suffix);
    Ok(result)
}

fn sense_code(value: &str) -> io::Result<u8> {
    match value {
        "unknown" => Ok(0),
        "generic" => Ok(1),
        "reference" => Ok(2),
        "haplotype" => Ok(3),
        _ => Err(invalid(format!("unknown path sense {value}"))),
    }
}

fn sense_name(value: u8) -> io::Result<&'static str> {
    match value {
        0 => Ok("unknown"),
        1 => Ok("generic"),
        2 => Ok("reference"),
        3 => Ok("haplotype"),
        _ => Err(invalid(format!("unknown path sense code {value}"))),
    }
}

fn encode_page(first_path_id: u64, paths: &[CatalogPath]) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    output.extend_from_slice(PAGE_MAGIC);
    put_u64(&mut output, first_path_id);
    put_u32(
        &mut output,
        u32::try_from(paths.len()).map_err(|_| invalid("page record count exceeds u32"))?,
    );
    put_u32(&mut output, 0);
    let mut previous_raw = String::new();
    let mut previous_sample = String::new();
    let mut previous_contig = String::new();
    for (index, path) in paths.iter().enumerate() {
        if path.canonical_path_id != first_path_id + index as u64 {
            return Err(invalid("catalog path IDs are not contiguous"));
        }
        encode_string(&mut output, &previous_raw, &path.raw_name);
        encode_string(&mut output, &previous_sample, &path.sample);
        encode_string(&mut output, &previous_contig, &path.contig);
        put_varint(&mut output, path.haplotype);
        put_varint(&mut output, path.fragment);
        output.push(sense_code(&path.path_sense)?);
        previous_raw.clone_from(&path.raw_name);
        previous_sample.clone_from(&path.sample);
        previous_contig.clone_from(&path.contig);
    }
    Ok(output)
}

fn decode_page(
    input: &[u8],
    expected_first: u64,
    expected_count: usize,
) -> io::Result<Vec<CatalogPath>> {
    if input.get(..8) != Some(PAGE_MAGIC) {
        return Err(invalid("invalid catalog page magic"));
    }
    let first_path_id = get_u64(input, 8)?;
    let record_count = usize::try_from(get_u32(input, 16)?)
        .map_err(|_| invalid("page record count does not fit in usize"))?;
    if first_path_id != expected_first || record_count != expected_count {
        return Err(invalid("catalog page identity differs from directory"));
    }
    if get_u32(input, 20)? != 0 {
        return Err(invalid("catalog page reserved bytes are nonzero"));
    }
    let mut cursor = 24_usize;
    let mut previous_raw = String::new();
    let mut previous_sample = String::new();
    let mut previous_contig = String::new();
    let mut result = Vec::with_capacity(record_count);
    for index in 0..record_count {
        let raw_name = decode_string(input, &mut cursor, &previous_raw)?;
        let sample = decode_string(input, &mut cursor, &previous_sample)?;
        let contig = decode_string(input, &mut cursor, &previous_contig)?;
        let haplotype = get_varint(input, &mut cursor)?;
        let fragment = get_varint(input, &mut cursor)?;
        let sense = *input
            .get(cursor)
            .ok_or_else(|| invalid("truncated path sense"))?;
        cursor += 1;
        let path = CatalogPath {
            canonical_path_id: first_path_id + index as u64,
            raw_name,
            sample,
            contig,
            haplotype,
            fragment,
            path_sense: sense_name(sense)?.to_owned(),
        };
        previous_raw.clone_from(&path.raw_name);
        previous_sample.clone_from(&path.sample);
        previous_contig.clone_from(&path.contig);
        result.push(path);
    }
    if cursor != input.len() {
        return Err(invalid("catalog page has trailing bytes"));
    }
    Ok(result)
}

fn read_catalog_paths(path: &Path) -> io::Result<Vec<CatalogPath>> {
    let input = BufReader::new(File::open(path)?);
    let mut declared_paths = None;
    let mut result = Vec::new();
    for line in input.lines() {
        let value: Value = serde_json::from_str(&line?)?;
        match value.get("type").and_then(Value::as_str) {
            Some("metadata") => declared_paths = value.get("paths").and_then(Value::as_u64),
            Some("path") => result.push(serde_json::from_value::<CatalogPath>(value)?),
            _ => {}
        }
    }
    if declared_paths != Some(result.len() as u64) {
        return Err(invalid("catalog metadata path count mismatch"));
    }
    result.sort_by_key(|path| path.canonical_path_id);
    for (index, path) in result.iter().enumerate() {
        if path.canonical_path_id != index as u64 {
            return Err(invalid("catalog path IDs are not contiguous from zero"));
        }
    }
    Ok(result)
}

#[derive(Clone, Debug)]
struct EncodedPage {
    record_count: u32,
    codec: u8,
    encoded: Vec<u8>,
    decoded_len: u64,
    digest: [u8; 16],
}

#[derive(Clone, Copy, Debug)]
struct PageEntry {
    record_count: u32,
    codec: u8,
    offset: u64,
    encoded_len: u64,
    decoded_len: u64,
    digest: [u8; 16],
}

fn encode_directory(entries: &[PageEntry]) -> Vec<u8> {
    let mut output = Vec::with_capacity(entries.len() * DIRECTORY_ENTRY_BYTES);
    for entry in entries {
        put_u32(&mut output, entry.record_count);
        output.push(entry.codec);
        output.extend_from_slice(&[0, 0, 0]);
        put_u64(&mut output, entry.offset);
        put_u64(&mut output, entry.encoded_len);
        put_u64(&mut output, entry.decoded_len);
        output.extend_from_slice(&entry.digest);
    }
    output
}

fn encode_header(
    records_per_page: u32,
    path_count: u64,
    page_count: u64,
    directory: &[u8],
    file_bytes: u64,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(HEADER_BYTES);
    output.extend_from_slice(CATALOG_MAGIC);
    put_u32(&mut output, CATALOG_VERSION);
    put_u32(&mut output, records_per_page);
    put_u64(&mut output, path_count);
    put_u64(&mut output, page_count);
    put_u64(&mut output, directory.len() as u64);
    put_u64(&mut output, file_bytes);
    output.extend_from_slice(&blake3::hash(directory).as_bytes()[..16]);
    debug_assert_eq!(output.len(), HEADER_BYTES);
    output
}

fn temporary_sibling(output: &Path) -> io::Result<PathBuf> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("output filename is not valid UTF-8"))?;
    Ok(output.with_file_name(format!(".{name}.tmp")))
}

fn write_atomic(
    output: &Path,
    header: &[u8],
    directory: &[u8],
    pages: &[EncodedPage],
) -> io::Result<()> {
    if output.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "paged catalog output already exists",
        ));
    }
    let temporary = temporary_sibling(output)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(header)?;
        file.write_all(directory)?;
        for page in pages {
            file.write_all(&page.encoded)?;
        }
        file.sync_all()
    })();
    if let Err(error) = result {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, output) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct BuildStats {
    schema_version: u32,
    catalog: String,
    output: String,
    path_count: usize,
    records_per_page: usize,
    page_count: usize,
    header_bytes: usize,
    directory_bytes: usize,
    decoded_page_bytes: u64,
    encoded_page_bytes: u64,
    output_bytes: u64,
    none_pages: usize,
    zstd_pages: usize,
    build_wall_ms: f64,
}

pub fn build_paged_catalog(values: &HashMap<String, String>) -> io::Result<()> {
    let started = Instant::now();
    let catalog_path = Path::new(required(values, "--catalog")?);
    let output_path = Path::new(required(values, "--output")?);
    let records_per_page = parse_usize(values, "--records-per-page")?;
    if records_per_page == 0 || records_per_page > MAX_RECORDS_PER_PAGE {
        return Err(invalid("--records-per-page must be between 1 and 65536"));
    }
    let paths = read_catalog_paths(catalog_path)?;
    let mut pages = Vec::new();
    for (page_id, chunk) in paths.chunks(records_per_page).enumerate() {
        let raw = encode_page((page_id * records_per_page) as u64, chunk)?;
        if raw.len() > MAX_PAGE_BYTES {
            return Err(invalid("decoded catalog page exceeds the 64 MiB limit"));
        }
        let compressed = zstd::stream::encode_all(Cursor::new(&raw), 3)?;
        let (codec, encoded) = if compressed.len() < raw.len() {
            (CODEC_ZSTD_3, compressed)
        } else {
            (CODEC_NONE, raw.clone())
        };
        let mut digest = [0_u8; 16];
        digest.copy_from_slice(&blake3::hash(&encoded).as_bytes()[..16]);
        pages.push(EncodedPage {
            record_count: u32::try_from(chunk.len())
                .map_err(|_| invalid("page record count exceeds u32"))?,
            codec,
            encoded,
            decoded_len: raw.len() as u64,
            digest,
        });
    }
    let directory_bytes = pages
        .len()
        .checked_mul(DIRECTORY_ENTRY_BYTES)
        .ok_or_else(|| invalid("catalog directory size overflow"))?;
    let mut next_offset = u64::try_from(HEADER_BYTES + directory_bytes)
        .map_err(|_| invalid("catalog root size does not fit in u64"))?;
    let mut entries = Vec::with_capacity(pages.len());
    for page in &pages {
        let encoded_len = page.encoded.len() as u64;
        entries.push(PageEntry {
            record_count: page.record_count,
            codec: page.codec,
            offset: next_offset,
            encoded_len,
            decoded_len: page.decoded_len,
            digest: page.digest,
        });
        next_offset = next_offset
            .checked_add(encoded_len)
            .ok_or_else(|| invalid("catalog file size overflow"))?;
    }
    let directory = encode_directory(&entries);
    let header = encode_header(
        u32::try_from(records_per_page).unwrap(),
        paths.len() as u64,
        pages.len() as u64,
        &directory,
        next_offset,
    );
    write_atomic(output_path, &header, &directory, &pages)?;
    let result = BuildStats {
        schema_version: 1,
        catalog: catalog_path.display().to_string(),
        output: output_path.display().to_string(),
        path_count: paths.len(),
        records_per_page,
        page_count: pages.len(),
        header_bytes: HEADER_BYTES,
        directory_bytes,
        decoded_page_bytes: pages.iter().map(|page| page.decoded_len).sum(),
        encoded_page_bytes: pages.iter().map(|page| page.encoded.len() as u64).sum(),
        output_bytes: next_offset,
        none_pages: pages.iter().filter(|page| page.codec == CODEC_NONE).count(),
        zstd_pages: pages
            .iter()
            .filter(|page| page.codec == CODEC_ZSTD_3)
            .count(),
        build_wall_ms: started.elapsed().as_secs_f64() * 1_000.0,
    };
    fs::write(
        required(values, "--stats")?,
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[derive(Debug)]
struct PagedCatalog {
    source: FileRangeSource,
    file_bytes: u64,
    path_count: usize,
    records_per_page: usize,
    root_bytes: usize,
    entries: Vec<PageEntry>,
}

impl PagedCatalog {
    #[allow(clippy::too_many_lines)]
    fn open(path: &Path) -> io::Result<Self> {
        let source = FileRangeSource::open(path)?;
        let file_bytes = source.len()?;
        if file_bytes < HEADER_BYTES as u64 {
            return Err(invalid("truncated paged catalog header"));
        }
        let header = source.read_range(0, HEADER_BYTES)?;
        if header.get(..8) != Some(CATALOG_MAGIC) {
            return Err(invalid("invalid paged catalog magic"));
        }
        if get_u32(&header, 8)? != CATALOG_VERSION {
            return Err(invalid("unsupported paged catalog version"));
        }
        let records_per_page = usize::try_from(get_u32(&header, 12)?)
            .map_err(|_| invalid("records per page does not fit in usize"))?;
        if records_per_page == 0 || records_per_page > MAX_RECORDS_PER_PAGE {
            return Err(invalid("paged catalog records per page are out of bounds"));
        }
        let path_count = usize_from_u64(get_u64(&header, 16)?, "path count")?;
        let page_count = usize_from_u64(get_u64(&header, 24)?, "page count")?;
        let directory_bytes = usize_from_u64(get_u64(&header, 32)?, "directory length")?;
        if get_u64(&header, 40)? != file_bytes {
            return Err(invalid("paged catalog file length mismatch"));
        }
        let expected_pages = path_count.div_ceil(records_per_page);
        if page_count != expected_pages
            || directory_bytes
                != page_count
                    .checked_mul(DIRECTORY_ENTRY_BYTES)
                    .ok_or_else(|| invalid("directory size overflow"))?
        {
            return Err(invalid("paged catalog directory count mismatch"));
        }
        let root_bytes = HEADER_BYTES
            .checked_add(directory_bytes)
            .ok_or_else(|| invalid("catalog root size overflow"))?;
        if root_bytes as u64 > file_bytes {
            return Err(invalid("paged catalog directory extends beyond file"));
        }
        let directory = source.read_range(HEADER_BYTES as u64, directory_bytes)?;
        if blake3::hash(&directory).as_bytes()[..16] != header[48..64] {
            return Err(invalid("paged catalog directory digest mismatch"));
        }
        let mut entries = Vec::with_capacity(page_count);
        let mut expected_offset = root_bytes as u64;
        for page_id in 0..page_count {
            let offset = page_id * DIRECTORY_ENTRY_BYTES;
            let record_count = get_u32(&directory, offset)?;
            let codec = directory[offset + 4];
            if directory[offset + 5..offset + 8] != [0, 0, 0] {
                return Err(invalid(
                    "paged catalog directory reserved bytes are nonzero",
                ));
            }
            if codec != CODEC_NONE && codec != CODEC_ZSTD_3 {
                return Err(invalid("paged catalog has an unknown codec"));
            }
            let page_offset = get_u64(&directory, offset + 8)?;
            let encoded_len = get_u64(&directory, offset + 16)?;
            let decoded_len = get_u64(&directory, offset + 24)?;
            let expected_count = if page_id + 1 == page_count {
                path_count - page_id * records_per_page
            } else {
                records_per_page
            };
            if record_count as usize != expected_count
                || encoded_len == 0
                || encoded_len > MAX_PAGE_BYTES as u64
                || decoded_len < 24
                || decoded_len > MAX_PAGE_BYTES as u64
            {
                return Err(invalid("paged catalog page dimensions are invalid"));
            }
            if page_offset != expected_offset {
                return Err(invalid("paged catalog pages are not contiguous"));
            }
            expected_offset = expected_offset
                .checked_add(encoded_len)
                .ok_or_else(|| invalid("paged catalog payload range overflow"))?;
            if expected_offset > file_bytes {
                return Err(invalid("paged catalog page extends beyond file"));
            }
            let mut digest = [0_u8; 16];
            digest.copy_from_slice(&directory[offset + 32..offset + 48]);
            entries.push(PageEntry {
                record_count,
                codec,
                offset: page_offset,
                encoded_len,
                decoded_len,
                digest,
            });
        }
        if expected_offset != file_bytes {
            return Err(invalid("paged catalog has trailing bytes"));
        }
        Ok(Self {
            source,
            file_bytes,
            path_count,
            records_per_page,
            root_bytes,
            entries,
        })
    }

    fn decode_encoded_page(&self, page_id: usize, encoded: &[u8]) -> io::Result<Vec<CatalogPath>> {
        let entry = self
            .entries
            .get(page_id)
            .ok_or_else(|| invalid("page ID is out of range"))?;
        if encoded.len() as u64 != entry.encoded_len {
            return Err(invalid("catalog page encoded length mismatch"));
        }
        if blake3::hash(encoded).as_bytes()[..16] != entry.digest {
            return Err(invalid("catalog page digest mismatch"));
        }
        let raw = match entry.codec {
            CODEC_NONE => encoded.to_vec(),
            CODEC_ZSTD_3 => {
                let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(encoded))?;
                decoder.window_log_max(26)?;
                let mut limited = decoder.take(MAX_PAGE_BYTES as u64 + 1);
                let mut raw =
                    Vec::with_capacity(usize_from_u64(entry.decoded_len, "decoded page length")?);
                limited.read_to_end(&mut raw)?;
                if raw.len() > MAX_PAGE_BYTES {
                    return Err(invalid("decoded catalog page exceeds the 64 MiB limit"));
                }
                raw
            }
            _ => return Err(invalid("catalog page codec is unsupported")),
        };
        if raw.len() as u64 != entry.decoded_len {
            return Err(invalid("catalog page decoded length mismatch"));
        }
        decode_page(
            &raw,
            (page_id * self.records_per_page) as u64,
            entry.record_count as usize,
        )
    }

    fn read_page(&self, page_id: usize) -> io::Result<Vec<CatalogPath>> {
        let entry = self
            .entries
            .get(page_id)
            .ok_or_else(|| invalid("page ID is out of range"))?;
        let encoded = self.source.read_range(
            entry.offset,
            usize_from_u64(entry.encoded_len, "encoded page length")?,
        )?;
        self.decode_encoded_page(page_id, &encoded)
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ByteRange {
    offset: u64,
    length: u64,
}

fn plan_ranges(
    catalog: &PagedCatalog,
    page_ids: &[usize],
    max_ranges: usize,
) -> io::Result<Vec<ByteRange>> {
    if max_ranges == 0 {
        return Err(invalid("max data ranges must be at least one"));
    }
    let mut ranges = page_ids
        .iter()
        .map(|&page_id| {
            let entry = catalog
                .entries
                .get(page_id)
                .ok_or_else(|| invalid("page ID is out of range"))?;
            Ok(ByteRange {
                offset: entry.offset,
                length: entry.encoded_len,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    while ranges.len() > max_ranges {
        let (merge_at, _) = ranges
            .windows(2)
            .enumerate()
            .map(|(index, pair)| {
                let left_end = pair[0].offset + pair[0].length;
                (index, pair[1].offset - left_end)
            })
            .min_by_key(|(index, gap)| (*gap, *index))
            .ok_or_else(|| invalid("cannot merge an empty range plan"))?;
        let right = ranges.remove(merge_at + 1);
        ranges[merge_at].length = right.offset + right.length - ranges[merge_at].offset;
    }
    Ok(ranges)
}

fn read_query_ids(path: &Path, path_count: usize) -> io::Result<Vec<u64>> {
    let input = BufReader::new(File::open(path)?);
    let mut result = BTreeSet::new();
    for line in input.lines() {
        let line = line?;
        let value = line.trim();
        if value.is_empty() {
            continue;
        }
        let id: u64 = value
            .parse()
            .map_err(|_| invalid("invalid query path ID"))?;
        if id >= path_count as u64 {
            return Err(invalid(format!(
                "query path ID {id} is outside the catalog"
            )));
        }
        result.insert(id);
    }
    if result.is_empty() {
        return Err(invalid("query path ID file is empty"));
    }
    Ok(result.into_iter().collect())
}

#[derive(Debug, Serialize)]
struct VerifyStats {
    schema_version: u32,
    catalog: String,
    paged: String,
    paged_bytes: u64,
    path_count: usize,
    records_per_page: usize,
    page_count: usize,
    exhaustive_exact_matches: usize,
    query_path_ids: usize,
    query_exact_matches: usize,
    selected_pages: usize,
    root_ranges: usize,
    root_bytes: usize,
    max_data_ranges: usize,
    data_ranges: Vec<ByteRange>,
    data_range_bytes: u64,
    selected_page_bytes: u64,
    data_overfetch_bytes: u64,
    total_query_ranges: usize,
    dependency_rounds: usize,
    total_query_bytes: u64,
    exhaustive_verify_wall_ms: f64,
    query_wall_ms: f64,
}

#[allow(clippy::too_many_lines)]
pub fn verify_paged_catalog(values: &HashMap<String, String>) -> io::Result<()> {
    let source_path = Path::new(required(values, "--catalog")?);
    let paged_path = Path::new(required(values, "--paged")?);
    let expected = read_catalog_paths(source_path)?;
    let catalog = PagedCatalog::open(paged_path)?;
    if catalog.path_count != expected.len() {
        return Err(invalid(
            "paged catalog path count differs from source catalog",
        ));
    }

    let exhaustive_started = Instant::now();
    let mut checked = 0_usize;
    for page_id in 0..catalog.entries.len() {
        let decoded = catalog.read_page(page_id)?;
        let first = page_id * catalog.records_per_page;
        let end = first + decoded.len();
        if decoded != expected[first..end] {
            return Err(invalid(format!(
                "catalog page {page_id} differs from source metadata"
            )));
        }
        checked += decoded.len();
    }
    let exhaustive_verify_wall_ms = exhaustive_started.elapsed().as_secs_f64() * 1_000.0;

    let query_ids = read_query_ids(Path::new(required(values, "--query-ids")?), expected.len())?;
    let page_ids = query_ids
        .iter()
        .map(|id| Ok(usize_from_u64(*id, "query path ID")? / catalog.records_per_page))
        .collect::<io::Result<BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let max_data_ranges = parse_usize(values, "--max-data-ranges")?;
    if max_data_ranges == 0 || max_data_ranges > 64 {
        return Err(invalid("--max-data-ranges must be between 1 and 64"));
    }
    let ranges = plan_ranges(&catalog, &page_ids, max_data_ranges)?;
    let query_started = Instant::now();
    let fetched = ranges
        .iter()
        .map(|range| {
            Ok((
                *range,
                catalog.source.read_range(
                    range.offset,
                    usize_from_u64(range.length, "query range length")?,
                )?,
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut decoded_pages = BTreeMap::new();
    for page_id in &page_ids {
        let entry = catalog.entries[*page_id];
        let (range, bytes) = fetched
            .iter()
            .find(|(range, _)| {
                entry.offset >= range.offset
                    && entry.offset + entry.encoded_len <= range.offset + range.length
            })
            .ok_or_else(|| invalid("query plan does not cover a selected page"))?;
        let start = usize_from_u64(
            entry.offset - range.offset,
            "page offset within query range",
        )?;
        let end = start
            .checked_add(usize_from_u64(entry.encoded_len, "selected page length")?)
            .ok_or_else(|| invalid("selected page slice overflow"))?;
        decoded_pages.insert(
            *page_id,
            catalog.decode_encoded_page(*page_id, &bytes[start..end])?,
        );
    }
    for id in &query_ids {
        let id_usize = usize_from_u64(*id, "query path ID")?;
        let page_id = id_usize / catalog.records_per_page;
        let within_page = id_usize % catalog.records_per_page;
        let actual = decoded_pages
            .get(&page_id)
            .and_then(|page| page.get(within_page))
            .ok_or_else(|| invalid("decoded query result is missing"))?;
        if actual != &expected[id_usize] {
            return Err(invalid(format!("paged lookup differs for path ID {id}")));
        }
    }
    let query_wall_ms = query_started.elapsed().as_secs_f64() * 1_000.0;
    let data_range_bytes = ranges.iter().map(|range| range.length).sum::<u64>();
    let selected_page_bytes = page_ids
        .iter()
        .map(|page_id| catalog.entries[*page_id].encoded_len)
        .sum::<u64>();
    let result = VerifyStats {
        schema_version: 1,
        catalog: source_path.display().to_string(),
        paged: paged_path.display().to_string(),
        paged_bytes: catalog.file_bytes,
        path_count: catalog.path_count,
        records_per_page: catalog.records_per_page,
        page_count: catalog.entries.len(),
        exhaustive_exact_matches: checked,
        query_path_ids: query_ids.len(),
        query_exact_matches: query_ids.len(),
        selected_pages: page_ids.len(),
        root_ranges: 1,
        root_bytes: catalog.root_bytes,
        max_data_ranges,
        data_ranges: ranges.clone(),
        data_range_bytes,
        selected_page_bytes,
        data_overfetch_bytes: data_range_bytes - selected_page_bytes,
        total_query_ranges: 1 + ranges.len(),
        dependency_rounds: 2,
        total_query_bytes: catalog.root_bytes as u64 + data_range_bytes,
        exhaustive_verify_wall_ms,
        query_wall_ms,
    };
    fs::write(
        required(values, "--stats")?,
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub fn export_catalog(values: &HashMap<String, String>) -> io::Result<()> {
    let gbwt_path = Path::new(required(values, "--gbwt")?);
    let output_path = Path::new(required(values, "--output")?);
    if output_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "catalog output already exists",
        ));
    }
    let gbwt: GBWT = serialize::load_from(gbwt_path)?;
    let path_count = if gbwt.is_bidirectional() {
        gbwt.sequences() / 2
    } else {
        gbwt.sequences()
    };
    let reference_samples = gbwt
        .tags()
        .get(REFERENCE_SAMPLES_KEY)
        .map_or("", String::as_str);
    let references = reference_samples
        .split_whitespace()
        .collect::<BTreeSet<_>>();
    let metadata = gbwt.metadata().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "complete GBWT path, sample, and contig metadata is required for catalog export",
        )
    })?;
    if !metadata.has_path_names()
        || !metadata.has_sample_names()
        || !metadata.has_contig_names()
        || metadata.paths() != path_count
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "complete GBWT path, sample, and contig metadata is required for catalog export",
        ));
    }
    let temporary = temporary_sibling(output_path)?;
    let mut output = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?,
    );
    let result = (|| {
        serde_json::to_writer(
            &mut output,
            &json!({
                "type": "metadata",
                "sequences": gbwt.sequences(),
                "paths": path_count,
                "bidirectional": gbwt.is_bidirectional(),
                "gbwt_nodes": gbwt.len(),
                "reference_samples": reference_samples,
            }),
        )?;
        output.write_all(b"\n")?;
        for path_id in 0..path_count {
            let sequence_id = if gbwt.is_bidirectional() {
                support::encode_path(path_id, Orientation::Forward)
            } else {
                path_id
            };
            let name = metadata.path(path_id).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing GBWT path metadata")
            })?;
            let sample = metadata
                .sample(name.sample())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing GBWT sample name")
                })?
                .to_owned();
            let contig = metadata
                .contig(name.contig())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing GBWT contig name")
                })?
                .to_owned();
            let haplotype = if sample == GENERIC_SAMPLE {
                u64::from(GENERIC_HAPLOTYPE)
            } else {
                name.phase() as u64
            };
            let fragment = name.fragment() as u64;
            let mut raw_name = format!("{sample}#{haplotype}#{contig}");
            if fragment != 0 {
                let _ = write!(raw_name, "#fragment={fragment}");
            }
            let path_sense = if sample == GENERIC_SAMPLE {
                "generic"
            } else if references.contains(sample.as_str()) {
                "reference"
            } else {
                "haplotype"
            };
            serde_json::to_writer(
                &mut output,
                &json!({
                    "type": "path",
                    "sequence_id": sequence_id,
                    "canonical_path_id": path_id,
                    "sequence_orientation": "forward",
                    "raw_name": raw_name,
                    "sample": sample,
                    "contig": contig,
                    "haplotype": haplotype,
                    "fragment": fragment,
                    "path_sense": path_sense,
                }),
            )?;
            output.write_all(b"\n")?;
        }
        output.flush()
    })();
    if let Err(error) = result {
        drop(output);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(output);
    if let Err(error) = fs::rename(&temporary, output_path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    println!(
        "{}",
        json!({
            "schema_version": 1,
            "gbwt": gbwt_path,
            "output": output_path,
            "paths": path_count,
            "sequences": gbwt.sequences(),
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_bytes(expected: &[CatalogPath], records_per_page: usize) -> Vec<u8> {
        let mut pages = Vec::new();
        for (page_id, chunk) in expected.chunks(records_per_page).enumerate() {
            let raw = encode_page((page_id * records_per_page) as u64, chunk).unwrap();
            let mut digest = [0_u8; 16];
            digest.copy_from_slice(&blake3::hash(&raw).as_bytes()[..16]);
            pages.push(EncodedPage {
                record_count: u32::try_from(chunk.len()).unwrap(),
                codec: CODEC_NONE,
                decoded_len: raw.len() as u64,
                digest,
                encoded: raw,
            });
        }
        let root_bytes = HEADER_BYTES + pages.len() * DIRECTORY_ENTRY_BYTES;
        let mut next_offset = root_bytes as u64;
        let entries = pages
            .iter()
            .map(|page| {
                let entry = PageEntry {
                    record_count: page.record_count,
                    codec: page.codec,
                    offset: next_offset,
                    encoded_len: page.encoded.len() as u64,
                    decoded_len: page.decoded_len,
                    digest: page.digest,
                };
                next_offset += entry.encoded_len;
                entry
            })
            .collect::<Vec<_>>();
        let directory = encode_directory(&entries);
        let header = encode_header(
            u32::try_from(records_per_page).unwrap(),
            expected.len() as u64,
            pages.len() as u64,
            &directory,
            next_offset,
        );
        let mut result = header;
        result.extend_from_slice(&directory);
        for page in pages {
            result.extend_from_slice(&page.encoded);
        }
        result
    }

    fn paths(count: usize) -> Vec<CatalogPath> {
        (0..count)
            .map(|id| CatalogPath {
                canonical_path_id: id as u64,
                raw_name: format!("sample-{}#{}#chr{}", id % 3, id % 2, id % 5),
                sample: format!("sample-{}", id % 3),
                contig: format!("chr{}", id % 5),
                haplotype: (id % 2) as u64,
                fragment: (id / 7) as u64,
                path_sense: if id % 4 == 0 {
                    "reference"
                } else {
                    "haplotype"
                }
                .to_owned(),
            })
            .collect()
    }

    #[test]
    fn page_codec_round_trips_and_rejects_corruption() {
        let mut expected = paths(17);
        expected[1].sample = "échantillon".to_owned();
        expected[2].sample = "êchantillon".to_owned();
        let raw = encode_page(0, &expected).unwrap();
        assert_eq!(decode_page(&raw, 0, expected.len()).unwrap(), expected);

        let mut trailing = raw.clone();
        trailing.push(0);
        assert!(decode_page(&trailing, 0, expected.len()).is_err());

        let mut corrupt = raw;
        corrupt[0] ^= 1;
        assert!(decode_page(&corrupt, 0, expected.len()).is_err());
    }

    #[test]
    fn on_disk_catalog_rejects_directory_payload_and_length_corruption() {
        let expected = paths(17);
        let bytes = catalog_bytes(&expected, 8);
        let path = serialize::temp_file_name("paged-catalog-corruption");

        fs::write(&path, &bytes).unwrap();
        let catalog = PagedCatalog::open(&path).unwrap();
        assert_eq!(catalog.read_page(0).unwrap(), expected[..8]);

        let mut corrupt_directory = bytes.clone();
        corrupt_directory[HEADER_BYTES] ^= 1;
        fs::write(&path, corrupt_directory).unwrap();
        assert!(PagedCatalog::open(&path).is_err());

        let mut corrupt_payload = bytes.clone();
        let first_payload = HEADER_BYTES + 3 * DIRECTORY_ENTRY_BYTES;
        corrupt_payload[first_payload] ^= 1;
        fs::write(&path, corrupt_payload).unwrap();
        let catalog = PagedCatalog::open(&path).unwrap();
        assert!(catalog.read_page(0).is_err());

        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(PagedCatalog::open(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn range_planner_respects_the_budget() {
        let catalog = PagedCatalog {
            source: FileRangeSource::open(support::get_test_data("example.gbwt")).unwrap(),
            file_bytes: 1_000,
            path_count: 40,
            records_per_page: 10,
            root_bytes: 64,
            entries: vec![
                PageEntry {
                    record_count: 10,
                    codec: 0,
                    offset: 100,
                    encoded_len: 10,
                    decoded_len: 24,
                    digest: [0; 16],
                },
                PageEntry {
                    record_count: 10,
                    codec: 0,
                    offset: 110,
                    encoded_len: 10,
                    decoded_len: 24,
                    digest: [0; 16],
                },
                PageEntry {
                    record_count: 10,
                    codec: 0,
                    offset: 200,
                    encoded_len: 10,
                    decoded_len: 24,
                    digest: [0; 16],
                },
                PageEntry {
                    record_count: 10,
                    codec: 0,
                    offset: 500,
                    encoded_len: 10,
                    decoded_len: 24,
                    digest: [0; 16],
                },
            ],
        };
        let planned = plan_ranges(&catalog, &[0, 1, 2, 3], 2).unwrap();
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].offset, 100);
        assert_eq!(planned[0].length, 110);
        assert_eq!(planned[1].offset, 500);
    }
}

use pangenome_range_format::{
    ARCHIVE_METADATA_TYPE_ID, ArchiveMetadata, ChunkCodec, ExtensionEntry, ExtensionPage,
    LocusPageDescriptor, LocusRecord, MAX_FEATURE_PAGE_BYTES, MAX_LOCUS_RECORDS_PER_PAGE,
    NAMED_LOCI_TYPE_ID, NamedLociDescriptor, ReferenceManifest, SUMMARY_PYRAMID_TYPE_ID,
    SummaryBin, SummaryPyramidDescriptor, SummarySeriesDescriptor, compress,
    encode_archive_metadata, encode_locus_page, encode_named_loci_descriptor,
    encode_summary_descriptor, encode_summary_page, normalize_locus_key,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const SUMMARY_WINDOW_MULTIPLIER: u64 = 64;
const SUMMARY_LEVEL_MULTIPLIER: u64 = 4;
const LOCUS_TARGET_RECORDS_PER_PAGE: usize = 256;
const NAMED_LOCUS_GFF3_FEATURE: &str = "gene";

#[derive(Clone, Debug, Default)]
pub(crate) struct FeatureBuildOptions {
    pub annotation_path: Option<PathBuf>,
    pub annotation_sample: Option<String>,
    pub annotation_feature_types: Vec<String>,
    pub archive_metadata: Option<ArchiveMetadata>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FeatureBuildMetrics {
    pub annotation_input_bytes: u64,
    pub annotation_feature_rows: u64,
    pub annotation_expanded_records: u64,
    pub named_locus_records: u64,
    pub named_locus_pages: u64,
    pub named_locus_page_encoded_bytes: u64,
    pub named_locus_page_decoded_bytes: u64,
    pub named_locus_descriptor_encoded_bytes: u64,
    pub annotation_checksum_wall_ms: f64,
    pub annotation_parse_expand_wall_ms: f64,
    pub annotation_sort_dedup_wall_ms: f64,
    pub named_locus_page_build_wall_ms: f64,
    pub named_locus_descriptor_wall_ms: f64,
    pub summary_series: u64,
    pub summary_bins: u64,
    pub extension_encoded_bytes: u64,
    pub extension_decoded_bytes: u64,
    pub path_membership_catalog_records: u64,
    pub path_membership_catalog_pages: u64,
    pub path_membership_directory_pages: u64,
    pub path_membership_tile_pages: u64,
    pub path_membership_groups: u64,
    pub path_membership_memberships: u64,
    pub path_membership_occurrence_total: u64,
    pub path_membership_unique_path_total: u64,
    pub path_membership_delta_groups: u64,
    pub path_membership_run_groups: u64,
    pub path_membership_page_encoded_bytes: u64,
    pub path_membership_page_decoded_bytes: u64,
    pub path_membership_descriptor_encoded_bytes: u64,
    pub path_membership_located_positions: u64,
    pub path_membership_maximum_lf_steps: u64,
    pub path_membership_locate_wall_ms: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BaseSummaryBins {
    bins: BTreeMap<(usize, u64), SummaryBin>,
}

impl BaseSummaryBins {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_tile(
        &mut self,
        manifest_index: usize,
        bin_span: u64,
        start: u64,
        end: u64,
        encoded_bytes: u64,
        decoded_bytes: u64,
        node_records: u64,
        edge_records: u64,
        gbwt_records: u64,
        occurrences: u64,
    ) -> io::Result<()> {
        if bin_span == 0 || start >= end {
            return Err(invalid_data("invalid base summary tile"));
        }
        let bin_start = (start / bin_span) * bin_span;
        let bin_end = bin_start
            .checked_add(bin_span)
            .ok_or_else(|| invalid_data("summary bin end overflow"))?;
        if end > bin_end {
            return Err(invalid_data("tile crosses its base summary bin"));
        }
        let bin = self.bins.entry((manifest_index, bin_start)).or_default();
        add(&mut bin.covered_bases, end - start, "covered bases")?;
        add(&mut bin.tile_count, 1, "tile count")?;
        add(&mut bin.encoded_bytes, encoded_bytes, "encoded bytes")?;
        add(&mut bin.decoded_bytes, decoded_bytes, "decoded bytes")?;
        add(&mut bin.node_records, node_records, "node records")?;
        add(&mut bin.edge_records, edge_records, "edge records")?;
        add(&mut bin.gbwt_records, gbwt_records, "GBWT records")?;
        add(&mut bin.occurrences, occurrences, "occurrences")?;
        Ok(())
    }
}

pub(crate) fn summary_base_bin_span(window_size: u64) -> io::Result<u64> {
    window_size
        .checked_mul(SUMMARY_WINDOW_MULTIPLIER)
        .ok_or_else(|| invalid_data("summary base bin span overflow"))
}

pub(crate) fn write_default_feature_extensions(
    archive: &mut File,
    manifests: &[ReferenceManifest],
    base_bin_span: u64,
    base_bins: &BaseSummaryBins,
    options: &FeatureBuildOptions,
    data_offset: u64,
) -> io::Result<(Vec<ExtensionEntry>, FeatureBuildMetrics)> {
    let mut metrics = FeatureBuildMetrics::default();
    let (named_loci, annotation) =
        write_named_loci(archive, manifests, options, data_offset, &mut metrics)?;
    let summaries = write_summaries(
        archive,
        manifests,
        base_bin_span,
        base_bins,
        data_offset,
        &mut metrics,
    )?;
    let mut entries = vec![summaries];
    if let Some(named_loci) = named_loci {
        entries.push(named_loci);
    }
    if let Some(mut metadata) = options.archive_metadata.clone() {
        if let Some((filename, checksum)) = annotation {
            metadata.annotation_filename = Some(filename);
            metadata.annotation_sha256 = Some(checksum);
        }
        let descriptor = encode_archive_metadata(&metadata)?;
        metrics.extension_decoded_bytes = metrics
            .extension_decoded_bytes
            .checked_add(usize_to_u64(descriptor.len())?)
            .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
        entries.push(append_descriptor(
            archive,
            ARCHIVE_METADATA_TYPE_ID,
            &descriptor,
            &mut metrics,
            data_offset,
        )?);
    }
    entries.sort_by_key(|entry| entry.type_id);
    Ok((entries, metrics))
}

type NamedLociWriteResult = (Option<ExtensionEntry>, Option<(String, [u8; 32])>);

struct Gff3Records {
    records: Vec<LocusRecord>,
    accepted_feature_rows: u64,
    expanded_records: u64,
    parse_expand_wall_ms: f64,
    sort_dedup_wall_ms: f64,
}

#[allow(
    clippy::too_many_lines,
    reason = "the measured named-locus pipeline is intentionally kept contiguous and auditable"
)]
fn write_named_loci(
    archive: &mut File,
    manifests: &[ReferenceManifest],
    options: &FeatureBuildOptions,
    data_offset: u64,
    metrics: &mut FeatureBuildMetrics,
) -> io::Result<NamedLociWriteResult> {
    let Some(path) = &options.annotation_path else {
        if options.annotation_sample.is_some() {
            return Err(invalid_data(
                "annotation sample requires an annotation file",
            ));
        }
        return Ok((None, None));
    };
    let (records, annotation_sha256, annotation_name) = {
        let sample = resolve_annotation_sample(manifests, options.annotation_sample.as_deref())?;
        metrics.annotation_input_bytes = std::fs::metadata(path)?.len();
        let checksum_started = Instant::now();
        let checksum = file_sha256(path)?;
        metrics.annotation_checksum_wall_ms = checksum_started.elapsed().as_secs_f64() * 1_000.0;
        let feature_types = annotation_feature_types(&options.annotation_feature_types)?;
        let parsed = read_gff3(path, &sample, manifests, &feature_types)?;
        if parsed.records.is_empty() {
            return Err(invalid_data(format!(
                "annotation file {} has no named features matching sample {sample} and the encoded contigs",
                path.display()
            )));
        }
        metrics.annotation_feature_rows = parsed.accepted_feature_rows;
        metrics.annotation_expanded_records = parsed.expanded_records;
        metrics.annotation_parse_expand_wall_ms = parsed.parse_expand_wall_ms;
        metrics.annotation_sort_dedup_wall_ms = parsed.sort_dedup_wall_ms;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("annotations.gff3")
            .to_owned();
        (parsed.records, checksum, name)
    };

    let page_build_started = Instant::now();
    let mut pages = Vec::new();
    let mut page_start = 0;
    while page_start < records.len() {
        let mut page_end = (page_start + LOCUS_TARGET_RECORDS_PER_PAGE).min(records.len());
        while page_end < records.len()
            && records[page_end - 1].normalized_key == records[page_end].normalized_key
        {
            page_end += 1;
        }
        let page_records = &records[page_start..page_end];
        if page_records.len() > MAX_LOCUS_RECORDS_PER_PAGE {
            return Err(invalid_data(format!(
                "normalized locus key {:?} expands to {} records, exceeding the per-key/page limit of {}",
                page_records[0].normalized_key,
                page_records.len(),
                MAX_LOCUS_RECORDS_PER_PAGE
            )));
        }
        let raw = encode_locus_page(page_records)?;
        let storage = append_page(archive, ChunkCodec::Zstd3, &raw, data_offset)?;
        metrics.named_locus_page_encoded_bytes = metrics
            .named_locus_page_encoded_bytes
            .checked_add(storage.encoded_len)
            .ok_or_else(|| invalid_data("named-locus encoded page bytes overflow"))?;
        metrics.named_locus_page_decoded_bytes = metrics
            .named_locus_page_decoded_bytes
            .checked_add(usize_to_u64(raw.len())?)
            .ok_or_else(|| invalid_data("named-locus decoded page bytes overflow"))?;
        metrics.extension_encoded_bytes = metrics
            .extension_encoded_bytes
            .checked_add(storage.encoded_len)
            .ok_or_else(|| invalid_data("extension encoded byte count overflow"))?;
        pages.push(LocusPageDescriptor {
            first_key: page_records
                .first()
                .expect("nonempty page")
                .normalized_key
                .clone(),
            last_key: page_records
                .last()
                .expect("nonempty page")
                .normalized_key
                .clone(),
            record_count: usize_to_u64(page_records.len())?,
            storage,
        });
        metrics.named_locus_pages += 1;
        metrics.extension_decoded_bytes = metrics
            .extension_decoded_bytes
            .checked_add(usize_to_u64(raw.len())?)
            .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
        page_start = page_end;
    }
    metrics.named_locus_page_build_wall_ms = page_build_started.elapsed().as_secs_f64() * 1_000.0;
    metrics.named_locus_records = usize_to_u64(records.len())?;
    let descriptor_started = Instant::now();
    let descriptor = encode_named_loci_descriptor(&NamedLociDescriptor {
        annotation_sha256,
        annotation_name: annotation_name.clone(),
        record_count: metrics.named_locus_records,
        pages,
    })?;
    metrics.extension_decoded_bytes = metrics
        .extension_decoded_bytes
        .checked_add(usize_to_u64(descriptor.len())?)
        .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
    let entry = append_descriptor(
        archive,
        NAMED_LOCI_TYPE_ID,
        &descriptor,
        metrics,
        data_offset,
    )?;
    metrics.named_locus_descriptor_encoded_bytes = entry.encoded_len;
    metrics.named_locus_descriptor_wall_ms = descriptor_started.elapsed().as_secs_f64() * 1_000.0;
    Ok((Some(entry), Some((annotation_name, annotation_sha256))))
}

fn write_summaries(
    archive: &mut File,
    manifests: &[ReferenceManifest],
    base_bin_span: u64,
    base_bins: &BaseSummaryBins,
    data_offset: u64,
    metrics: &mut FeatureBuildMetrics,
) -> io::Result<ExtensionEntry> {
    let mut series = Vec::new();
    for (manifest_index, manifest) in manifests.iter().enumerate() {
        let mut bin_span = base_bin_span;
        let mut level = 0_u32;
        let mut first_bin_start = (manifest.start / bin_span) * bin_span;
        let mut bins = summary_base_level_bins(manifest_index, manifest, bin_span, base_bins)?;
        loop {
            let raw = encode_summary_page(
                usize_to_u32(manifest_index)?,
                level,
                bin_span,
                first_bin_start,
                &bins,
            )?;
            let storage = append_page(archive, ChunkCodec::Zstd3, &raw, data_offset)?;
            metrics.extension_encoded_bytes = metrics
                .extension_encoded_bytes
                .checked_add(storage.encoded_len)
                .ok_or_else(|| invalid_data("extension encoded byte count overflow"))?;
            series.push(SummarySeriesDescriptor {
                manifest_index: usize_to_u32(manifest_index)?,
                level,
                bin_span,
                first_bin_start,
                bin_count: usize_to_u64(bins.len())?,
                storage,
            });
            metrics.summary_series += 1;
            metrics.summary_bins = metrics
                .summary_bins
                .checked_add(usize_to_u64(bins.len())?)
                .ok_or_else(|| invalid_data("summary bin count overflow"))?;
            metrics.extension_decoded_bytes = metrics
                .extension_decoded_bytes
                .checked_add(usize_to_u64(raw.len())?)
                .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
            if bins.len() == 1 {
                break;
            }
            let next_span = bin_span
                .checked_mul(SUMMARY_LEVEL_MULTIPLIER)
                .ok_or_else(|| invalid_data("summary level span overflow"))?;
            let (next_first, next_bins) =
                aggregate_summary_level(manifest, first_bin_start, bin_span, next_span, &bins)?;
            bins = next_bins;
            bin_span = next_span;
            first_bin_start = next_first;
            level = level
                .checked_add(1)
                .ok_or_else(|| invalid_data("summary level overflow"))?;
        }
    }
    let descriptor = encode_summary_descriptor(&SummaryPyramidDescriptor {
        base_bin_span,
        series,
    })?;
    metrics.extension_decoded_bytes = metrics
        .extension_decoded_bytes
        .checked_add(usize_to_u64(descriptor.len())?)
        .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
    append_descriptor(
        archive,
        SUMMARY_PYRAMID_TYPE_ID,
        &descriptor,
        metrics,
        data_offset,
    )
}

fn aggregate_summary_level(
    manifest: &ReferenceManifest,
    child_first: u64,
    child_span: u64,
    parent_span: u64,
    children: &[SummaryBin],
) -> io::Result<(u64, Vec<SummaryBin>)> {
    let parent_first = (manifest.start / parent_span) * parent_span;
    let last = ((manifest.end - 1) / parent_span) * parent_span;
    let count = ((last - parent_first) / parent_span) + 1;
    let mut parents = vec![
        SummaryBin::default();
        usize::try_from(count)
            .map_err(|_| invalid_data("summary bin count does not fit usize"))?
    ];
    for (index, value) in children.iter().enumerate() {
        let child_start = child_first
            .checked_add(
                usize_to_u64(index)?
                    .checked_mul(child_span)
                    .ok_or_else(|| invalid_data("summary child offset overflow"))?,
            )
            .ok_or_else(|| invalid_data("summary child start overflow"))?;
        let parent_index = (child_start - parent_first) / parent_span;
        add_bin(
            parents
                .get_mut(
                    usize::try_from(parent_index)
                        .map_err(|_| invalid_data("summary parent index does not fit usize"))?,
                )
                .ok_or_else(|| invalid_data("summary parent index is out of range"))?,
            value,
        )?;
    }
    Ok((parent_first, parents))
}

fn summary_base_level_bins(
    manifest_index: usize,
    manifest: &ReferenceManifest,
    bin_span: u64,
    base_bins: &BaseSummaryBins,
) -> io::Result<Vec<SummaryBin>> {
    let first = (manifest.start / bin_span) * bin_span;
    let last = ((manifest.end - 1) / bin_span) * bin_span;
    let count = ((last - first) / bin_span) + 1;
    let mut bins = vec![
        SummaryBin::default();
        usize::try_from(count)
            .map_err(|_| invalid_data("summary bin count does not fit usize"))?
    ];
    for ((reference, base_start), value) in &base_bins.bins {
        if *reference != manifest_index {
            continue;
        }
        let index = (*base_start - first) / bin_span;
        add_bin(
            bins.get_mut(
                usize::try_from(index)
                    .map_err(|_| invalid_data("summary bin index does not fit usize"))?,
            )
            .ok_or_else(|| invalid_data("base summary bin is outside its reference"))?,
            value,
        )?;
    }
    Ok(bins)
}

fn add_bin(target: &mut SummaryBin, value: &SummaryBin) -> io::Result<()> {
    add(
        &mut target.covered_bases,
        value.covered_bases,
        "covered bases",
    )?;
    add(&mut target.tile_count, value.tile_count, "tile count")?;
    add(
        &mut target.encoded_bytes,
        value.encoded_bytes,
        "encoded bytes",
    )?;
    add(
        &mut target.decoded_bytes,
        value.decoded_bytes,
        "decoded bytes",
    )?;
    add(&mut target.node_records, value.node_records, "node records")?;
    add(&mut target.edge_records, value.edge_records, "edge records")?;
    add(&mut target.gbwt_records, value.gbwt_records, "GBWT records")?;
    add(&mut target.occurrences, value.occurrences, "occurrences")
}

pub(crate) fn append_descriptor(
    archive: &mut File,
    type_id: [u8; 16],
    bytes: &[u8],
    metrics: &mut FeatureBuildMetrics,
    data_offset: u64,
) -> io::Result<ExtensionEntry> {
    let offset = archive.seek(SeekFrom::End(0))?;
    if offset < data_offset {
        return Err(invalid_data(
            "extension descriptor starts before archive data",
        ));
    }
    archive.write_all(bytes)?;
    let digest = blake3::hash(bytes);
    metrics.extension_encoded_bytes = metrics
        .extension_encoded_bytes
        .checked_add(usize_to_u64(bytes.len())?)
        .ok_or_else(|| invalid_data("extension encoded byte count overflow"))?;
    Ok(ExtensionEntry {
        type_id,
        required: false,
        codec: ChunkCodec::None,
        offset,
        encoded_len: usize_to_u64(bytes.len())?,
        decoded_len: usize_to_u64(bytes.len())?,
        integrity: digest.as_bytes()[..16]
            .try_into()
            .expect("fixed BLAKE3 digest"),
    })
}

pub(crate) fn append_page(
    archive: &mut File,
    codec: ChunkCodec,
    raw: &[u8],
    data_offset: u64,
) -> io::Result<ExtensionPage> {
    let encoded = compress(codec, raw)?;
    if usize_to_u64(raw.len())? > MAX_FEATURE_PAGE_BYTES
        || usize_to_u64(encoded.len())? > MAX_FEATURE_PAGE_BYTES
    {
        return Err(invalid_data(format!(
            "extension page exceeds the {MAX_FEATURE_PAGE_BYTES}-byte encoded/decoded limit"
        )));
    }
    let offset = archive.seek(SeekFrom::End(0))?;
    if offset < data_offset {
        return Err(invalid_data("extension page starts before archive data"));
    }
    archive.write_all(&encoded)?;
    let digest = blake3::hash(&encoded);
    Ok(ExtensionPage {
        offset,
        encoded_len: usize_to_u64(encoded.len())?,
        decoded_len: usize_to_u64(raw.len())?,
        codec,
        integrity: digest.as_bytes()[..16]
            .try_into()
            .expect("fixed BLAKE3 digest"),
    })
}

fn resolve_annotation_sample(
    manifests: &[ReferenceManifest],
    requested: Option<&str>,
) -> io::Result<String> {
    let samples = manifests
        .iter()
        .map(|manifest| manifest.sample.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(requested) = requested {
        if !samples.contains(requested) {
            return Err(invalid_data(format!(
                "annotation sample {requested} is not present in the encoded references"
            )));
        }
        return Ok(requested.to_owned());
    }
    if samples.len() != 1 {
        return Err(invalid_data(
            "--annotation-sample is required when the archive contains multiple reference samples",
        ));
    }
    Ok((*samples.first().expect("one sample")).to_owned())
}

#[allow(
    clippy::too_many_lines,
    reason = "GFF3 directive, coverage, and ordering validation form one fail-closed parser"
)]
fn read_gff3(
    path: &Path,
    sample: &str,
    manifests: &[ReferenceManifest],
    feature_types: &BTreeSet<String>,
) -> io::Result<Gff3Records> {
    let parse_started = Instant::now();
    let file = File::open(path)?;
    let intervals = manifests
        .iter()
        .filter(|manifest| manifest.sample == sample)
        .fold(
            BTreeMap::<&str, Vec<(u64, u64)>>::new(),
            |mut map, manifest| {
                map.entry(&manifest.contig)
                    .or_default()
                    .push((manifest.start, manifest.end));
                map
            },
        );
    let mut records = Vec::new();
    let mut accepted_feature_rows = 0_u64;
    let mut sequence_regions = BTreeMap::<String, (u64, u64)>::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if let Some(directive) = line.strip_prefix("##sequence-region") {
            let fields = directive.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(invalid_data(format!(
                    "invalid GFF3 ##sequence-region directive at line {}",
                    line_index + 1
                )));
            }
            let start = fields[1].parse::<u64>().map_err(|_| {
                invalid_data(format!(
                    "invalid GFF3 sequence-region start at line {}",
                    line_index + 1
                ))
            })?;
            let end = fields[2].parse::<u64>().map_err(|_| {
                invalid_data(format!(
                    "invalid GFF3 sequence-region end at line {}",
                    line_index + 1
                ))
            })?;
            if start == 0 || end < start {
                return Err(invalid_data(format!(
                    "invalid GFF3 sequence-region interval at line {}",
                    line_index + 1
                )));
            }
            if let Some(previous) = sequence_regions.insert(fields[0].to_owned(), (start, end))
                && previous != (start, end)
            {
                return Err(invalid_data(format!(
                    "conflicting GFF3 sequence-region for {} at line {}",
                    fields[0],
                    line_index + 1
                )));
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let expanded =
            gff3_records_for_line(&line, line_index + 1, sample, &intervals, feature_types)?;
        if !expanded.is_empty() {
            accepted_feature_rows = accepted_feature_rows
                .checked_add(1)
                .ok_or_else(|| invalid_data("accepted GFF3 feature row count overflow"))?;
        }
        records.extend(expanded);
    }
    for record in &records {
        if let Some((region_start, region_end)) = sequence_regions.get(&record.contig)
            && (record.start + 1 < *region_start || record.end > *region_end)
        {
            return Err(invalid_data(format!(
                "GFF3 feature {}:{}-{} lies outside ##sequence-region {}-{}",
                record.contig,
                record.start + 1,
                record.end,
                region_start,
                region_end
            )));
        }
    }
    let parse_expand_wall_ms = parse_started.elapsed().as_secs_f64() * 1_000.0;
    let expanded_records = usize_to_u64(records.len())?;
    let sort_started = Instant::now();
    records.sort_by(|left, right| {
        (
            &left.normalized_key,
            &left.sample,
            &left.contig,
            left.start,
            left.end,
            &left.stable_id,
        )
            .cmp(&(
                &right.normalized_key,
                &right.sample,
                &right.contig,
                right.start,
                right.end,
                &right.stable_id,
            ))
    });
    records.dedup();
    Ok(Gff3Records {
        records,
        accepted_feature_rows,
        expanded_records,
        parse_expand_wall_ms,
        sort_dedup_wall_ms: sort_started.elapsed().as_secs_f64() * 1_000.0,
    })
}

fn gff3_records_for_line(
    line: &str,
    line_number: usize,
    sample: &str,
    intervals: &BTreeMap<&str, Vec<(u64, u64)>>,
    feature_types: &BTreeSet<String>,
) -> io::Result<Vec<LocusRecord>> {
    let columns = line.split('\t').collect::<Vec<_>>();
    if columns.len() != 9 {
        return Err(invalid_data(format!(
            "invalid GFF3 column count at line {line_number}"
        )));
    }
    if !feature_types.contains(columns[2]) {
        return Ok(Vec::new());
    }
    let Some(reference_intervals) = intervals.get(columns[0]) else {
        return Ok(Vec::new());
    };
    let one_based_start = columns[3]
        .parse::<u64>()
        .map_err(|_| invalid_data(format!("invalid GFF3 start at line {line_number}")))?;
    let end = columns[4]
        .parse::<u64>()
        .map_err(|_| invalid_data(format!("invalid GFF3 end at line {line_number}")))?;
    if one_based_start == 0 || end < one_based_start {
        return Err(invalid_data(format!(
            "invalid GFF3 interval at line {line_number}"
        )));
    }
    let start = one_based_start - 1;
    if !reference_intervals
        .iter()
        .any(|(left, right)| start >= *left && end <= *right)
    {
        return Ok(Vec::new());
    }
    let attributes = parse_gff3_attributes(columns[8], line_number)?;
    let stable_id = first_attribute(&attributes, &["ID", "gene_id"])
        .or_else(|| first_attribute(&attributes, &["Name", "gene_name"]))
        .unwrap_or_default();
    let display_name =
        first_attribute(&attributes, &["Name", "gene_name"]).unwrap_or_else(|| stable_id.clone());
    if stable_id.is_empty() || display_name.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = BTreeSet::new();
    for key in [
        "Name",
        "gene_name",
        "ID",
        "gene_id",
        "Alias",
        "gene_synonym",
    ] {
        if let Some(values) = attributes.get(key) {
            names.extend(
                values
                    .iter()
                    .filter(|value| !value.trim().is_empty())
                    .cloned(),
            );
        }
    }
    let strand = match columns[6] {
        "." | "?" => 0,
        "+" => 1,
        "-" => 2,
        _ => {
            return Err(invalid_data(format!(
                "invalid GFF3 strand at line {line_number}"
            )));
        }
    };
    Ok(names
        .into_iter()
        .filter_map(|matched_name| {
            let normalized_key = normalize_locus_key(&matched_name);
            (!normalized_key.is_empty()).then(|| LocusRecord {
                normalized_key,
                matched_name,
                display_name: display_name.clone(),
                stable_id: stable_id.clone(),
                feature_type: columns[2].to_owned(),
                sample: sample.to_owned(),
                contig: columns[0].to_owned(),
                start,
                end,
                strand,
            })
        })
        .collect())
}

fn annotation_feature_types(requested: &[String]) -> io::Result<BTreeSet<String>> {
    let feature_types = if requested.is_empty() {
        BTreeSet::from([NAMED_LOCUS_GFF3_FEATURE.to_owned()])
    } else {
        requested.iter().cloned().collect()
    };
    if feature_types.iter().any(|value| {
        value.is_empty()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte == b',' || byte == b';')
    }) {
        return Err(invalid_data(
            "annotation feature types must be nonempty exact GFF3 type tokens",
        ));
    }
    Ok(feature_types)
}

fn parse_gff3_attributes(
    value: &str,
    line_number: usize,
) -> io::Result<BTreeMap<String, Vec<String>>> {
    let mut result = BTreeMap::new();
    if value == "." {
        return Ok(result);
    }
    for field in value.split(';').filter(|field| !field.is_empty()) {
        let (key, raw_value) = field.split_once('=').ok_or_else(|| {
            invalid_data(format!(
                "invalid GFF3 attribute at line {line_number}; expected key=value"
            ))
        })?;
        let key = percent_decode(key, line_number)?;
        let mut values = Vec::new();
        for item in raw_value.split(',') {
            values.push(percent_decode(item, line_number)?);
        }
        result.entry(key).or_insert_with(Vec::new).extend(values);
    }
    Ok(result)
}

fn first_attribute(attributes: &BTreeMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        attributes
            .get(*key)
            .and_then(|values| values.iter().find(|value| !value.is_empty()))
            .cloned()
    })
}

fn percent_decode(value: &str, line_number: usize) -> io::Result<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(invalid_data(format!(
                    "truncated GFF3 percent escape at line {line_number}"
                )));
            }
            let high = hex(bytes[index + 1]).ok_or_else(|| {
                invalid_data(format!("invalid GFF3 percent escape at line {line_number}"))
            })?;
            let low = hex(bytes[index + 2]).ok_or_else(|| {
                invalid_data(format!("invalid GFF3 percent escape at line {line_number}"))
            })?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| {
        invalid_data(format!(
            "GFF3 attribute is not valid UTF-8 at line {line_number}"
        ))
    })
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn file_sha256(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

fn add(target: &mut u64, value: u64, label: &str) -> io::Result<()> {
    *target = target
        .checked_add(value)
        .ok_or_else(|| invalid_data(format!("summary {label} overflow")))?;
    Ok(())
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_data("usize does not fit u64"))
}

fn usize_to_u32(value: usize) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| invalid_data("usize does not fit u32"))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_gff3_attributes() {
        let attributes =
            parse_gff3_attributes("ID=gene%3A1;Name=BRCA1;Alias=RNF53,BRCC1", 1).unwrap();
        assert_eq!(attributes["ID"], ["gene:1"]);
        assert_eq!(attributes["Alias"], ["RNF53", "BRCC1"]);
        assert_eq!(normalize_locus_key(" BRCA1 "), "brca1");
    }

    #[test]
    fn named_loci_index_gene_features_without_child_feature_amplification() {
        let intervals = BTreeMap::from([("chr1", vec![(0, 1_000)])]);
        let attributes =
            "ID=ENSG00000000001.1;gene_id=ENSG00000000001.1;gene_name=BRCA1;Alias=RNF53,BRCC1";
        let feature_types = annotation_feature_types(&[]).unwrap();
        let gene = gff3_records_for_line(
            &format!("chr1\tGENCODE\tgene\t101\t200\t.\t+\t.\t{attributes}"),
            1,
            "GRCh38",
            &intervals,
            &feature_types,
        )
        .unwrap();
        assert_eq!(gene.len(), 4);
        assert!(gene.iter().all(|record| record.feature_type == "gene"));
        assert_eq!(
            gene.iter()
                .map(|record| record.matched_name.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["BRCA1", "BRCC1", "ENSG00000000001.1", "RNF53",])
        );

        for feature in [
            "transcript",
            "exon",
            "CDS",
            "five_prime_UTR",
            "three_prime_UTR",
            "start_codon",
            "stop_codon",
        ] {
            let child = gff3_records_for_line(
                &format!("chr1\tGENCODE\t{feature}\t101\t200\t.\t+\t.\t{attributes}"),
                2,
                "GRCh38",
                &intervals,
                &feature_types,
            )
            .unwrap();
            assert!(child.is_empty(), "unexpected {feature} locus records");
        }
    }

    #[test]
    fn aggregates_summary_bins_without_changing_totals() {
        let manifest = ReferenceManifest {
            sample: "ref".into(),
            contig: "chr1".into(),
            start: 1_100,
            end: 9_000,
            grid_start: 0,
            window_size: 100,
            bucket_span: 3_200,
            first_page_offset: 0,
            page_count: 1,
            entry_count: 2,
            codec: ChunkCodec::Zstd3,
        };
        let mut base = BaseSummaryBins::default();
        base.bins.insert(
            (0, 1_000),
            SummaryBin {
                covered_bases: 10,
                tile_count: 1,
                ..SummaryBin::default()
            },
        );
        base.bins.insert(
            (0, 2_000),
            SummaryBin {
                covered_bases: 20,
                tile_count: 2,
                ..SummaryBin::default()
            },
        );
        let parent = summary_base_level_bins(0, &manifest, 4_000, &base).unwrap();
        assert_eq!(parent[0].covered_bases, 30);
        assert_eq!(parent[0].tile_count, 3);
    }

    #[test]
    fn linear_summary_levels_preserve_every_tile_record_total() {
        let manifest = ReferenceManifest {
            sample: "ref".into(),
            contig: "fragment".into(),
            start: 1_100,
            end: 40_000,
            grid_start: 0,
            window_size: 100,
            bucket_span: 3_200,
            first_page_offset: 0,
            page_count: 1,
            entry_count: 8,
            codec: ChunkCodec::Zstd3,
        };
        let children = (0_u64..10)
            .map(|index| SummaryBin {
                covered_bases: index + 1,
                tile_count: 1,
                encoded_bytes: index + 2,
                decoded_bytes: index + 3,
                node_records: index + 4,
                edge_records: index + 5,
                gbwt_records: index + 6,
                occurrences: index + 7,
            })
            .collect::<Vec<_>>();
        let expected = children
            .iter()
            .fold(SummaryBin::default(), |mut total, bin| {
                add_bin(&mut total, bin).unwrap();
                total
            });
        let (parent_first, parents) =
            aggregate_summary_level(&manifest, 0, 4_000, 16_000, &children).unwrap();
        let actual = parents
            .iter()
            .fold(SummaryBin::default(), |mut total, bin| {
                add_bin(&mut total, bin).unwrap();
                total
            });
        assert_eq!(parent_first, 0);
        assert_eq!(actual, expected);
    }

    #[test]
    fn gff3_feature_types_and_partial_archive_policy_are_explicit() {
        let intervals = BTreeMap::from([("chr1", vec![(100, 200)])]);
        let types = annotation_feature_types(&["pseudogene".into(), "gene".into()]).unwrap();
        let attributes = "ID=gene%3A1;Name=BRCA1";
        let contained = gff3_records_for_line(
            &format!("chr1\ttest\tpseudogene\t101\t200\t.\t+\t.\t{attributes}"),
            1,
            "GRCh38",
            &intervals,
            &types,
        )
        .unwrap();
        assert!(!contained.is_empty());
        let overlapping = gff3_records_for_line(
            &format!("chr1\ttest\tgene\t100\t200\t.\t+\t.\t{attributes}"),
            2,
            "GRCh38",
            &intervals,
            &types,
        )
        .unwrap();
        assert!(overlapping.is_empty());
    }

    #[test]
    fn gff3_sequence_region_is_validated_even_when_directive_follows_records() {
        let path = simple_sds::serialize::temp_file_name("gff3-sequence-region");
        std::fs::write(
            &path,
            "chr1\ttest\tgene\t101\t200\t.\t+\t.\tID=gene1;Name=ONE\n##sequence-region chr1 1 150\n",
        )
        .unwrap();
        let manifests = vec![ReferenceManifest {
            sample: "GRCh38".into(),
            contig: "chr1".into(),
            start: 0,
            end: 1_000,
            grid_start: 0,
            window_size: 100,
            bucket_span: 3_200,
            first_page_offset: 0,
            page_count: 1,
            entry_count: 1,
            codec: ChunkCodec::Zstd3,
        }];
        let types = annotation_feature_types(&[]).unwrap();
        assert!(read_gff3(&path, "GRCh38", &manifests, &types).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

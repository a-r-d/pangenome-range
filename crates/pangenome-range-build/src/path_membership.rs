use crate::features::{FeatureBuildMetrics, append_descriptor, append_page};
use crate::source::{PangenomeSource, SourceLocatedPosition};
use gbz::Pos;
use pangenome_range_format::{
    ArchiveEntry, ChunkCodec, ExtensionEntry, PATH_CATALOG_RECORDS_PER_PAGE,
    PATH_MEMBERSHIP_DELTA_CODEC, PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES, PATH_MEMBERSHIP_RUN_CODEC,
    PATH_MEMBERSHIP_TYPE_ID, PathCatalogPageDescriptor, PathCatalogRecord, PathIdentitySource,
    PathMembership, PathMembershipDescriptor, PathMembershipDirectoryEntry, PathMembershipManifest,
    RecordRegionalPayload, ReferenceManifest, TraversalMembershipGroup, decompress,
    encode_path_catalog_page, encode_path_membership_descriptor,
    encode_path_membership_directory_page, encode_tile_membership_page,
    selected_path_membership_codec, traversal_membership_digest,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub(crate) struct PathMembershipBuildMetrics {
    pub catalog_records: u64,
    pub catalog_pages: u64,
    pub tile_pages: u64,
    pub directory_pages: u64,
    pub groups: u64,
    pub memberships: u64,
    pub occurrence_total: u64,
    pub unique_path_total: u64,
    pub delta_groups: u64,
    pub run_groups: u64,
    pub page_encoded_bytes: u64,
    pub page_decoded_bytes: u64,
    pub descriptor_encoded_bytes: u64,
    pub located_positions: u64,
    pub maximum_lf_steps: u64,
    pub locate_wall_ms: f64,
}

#[derive(Deserialize)]
struct InputSummary {
    manifest: InputManifest,
    tiles: Vec<InputTile>,
}

#[derive(Deserialize)]
struct InputManifest {
    sample: String,
    contig: String,
}

#[derive(Deserialize)]
struct InputTile {
    core_start: u64,
    core_end: u64,
    groups: Vec<InputGroup>,
}

#[derive(Deserialize)]
struct InputGroup {
    traversal: Vec<u64>,
    occurrence_weight: u64,
    unique_path_count: u64,
    memberships: Vec<InputMembership>,
}

#[derive(Deserialize)]
struct InputMembership {
    path_id: u64,
    multiplicity: u64,
    reversed_relative_to_group: bool,
}

#[derive(Deserialize)]
struct InputCatalogRecord {
    canonical_path_id: u64,
    raw_name: String,
    sample: String,
    contig: String,
    haplotype: u64,
    fragment: u64,
    path_sense: String,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn checked_add(target: &mut u64, value: u64, field: &str) -> io::Result<()> {
    *target = target
        .checked_add(value)
        .ok_or_else(|| invalid(format!("{field} overflow")))?;
    Ok(())
}

fn record_group_metrics(
    metrics: &mut PathMembershipBuildMetrics,
    groups: &[TraversalMembershipGroup],
) -> io::Result<()> {
    checked_add(
        &mut metrics.groups,
        usize_to_u64(groups.len())?,
        "membership group count",
    )?;
    for group in groups {
        checked_add(
            &mut metrics.memberships,
            usize_to_u64(group.memberships.len())?,
            "membership count",
        )?;
        checked_add(
            &mut metrics.occurrence_total,
            group.occurrence_weight,
            "membership occurrence total",
        )?;
        checked_add(
            &mut metrics.unique_path_total,
            group.unique_path_count,
            "membership unique-path total",
        )?;
        match selected_path_membership_codec(&group.memberships)? {
            PATH_MEMBERSHIP_DELTA_CODEC => {
                checked_add(&mut metrics.delta_groups, 1, "delta membership groups")?;
            }
            PATH_MEMBERSHIP_RUN_CODEC => {
                checked_add(&mut metrics.run_groups, 1, "run membership groups")?;
            }
            _ => return Err(invalid("unknown selected path-membership codec")),
        }
    }
    Ok(())
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid("usize does not fit u64"))
}

fn u64_to_usize(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid("u64 does not fit usize"))
}

#[derive(Clone, Debug)]
struct TraversalStart {
    position: Pos,
    traversal: Vec<u64>,
    is_reference: bool,
    canonical: bool,
}

struct TileMembershipIdentity<'a> {
    sample: &'a str,
    contig: &'a str,
    core_start: u64,
    core_end: u64,
    regional_payload_integrity: &'a [u8; 16],
}

fn canonical_edge(from: u64, to: u64) -> bool {
    let from_node = from / 2;
    let to_node = to / 2;
    let from_reverse = from % 2 != 0;
    let to_reverse = to % 2 != 0;
    from != 0
        && to != 0
        && if from_reverse {
            to_node > from_node || (to_node == from_node && !to_reverse)
        } else {
            to_node >= from_node
        }
}

fn path_is_canonical(path: &[u64]) -> bool {
    let Some((&first, rest)) = path.split_first() else {
        return true;
    };
    let last = *rest.last().unwrap_or(&first);
    if first % 2 == last % 2 {
        first % 2 == 0
    } else {
        canonical_edge(first, last)
    }
}

fn traversal_starts(payload: &RecordRegionalPayload) -> io::Result<Vec<TraversalStart>> {
    struct LocalRecord {
        handle: u64,
        successors: Vec<Pos>,
        has_predecessor: Vec<bool>,
    }
    let mut records = Vec::with_capacity(payload.records.len());
    for packed in &payload.records {
        let handle = u64_to_usize(packed.handle)?;
        let record = gbz::bwt::Record::new(handle, &packed.bytes)
            .ok_or_else(|| invalid("invalid packed GBWT record in regional payload"))?;
        let successors = record.decompress();
        if usize_to_u64(successors.len())? != packed.occurrence_count {
            return Err(invalid("regional GBWT occurrence count mismatch"));
        }
        records.push(LocalRecord {
            handle: packed.handle,
            has_predecessor: vec![false; successors.len()],
            successors,
        });
    }
    let record_by_handle = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.handle, index))
        .collect::<HashMap<_, _>>();
    for source in 0..records.len() {
        for offset in 0..records[source].successors.len() {
            let next = records[source].successors[offset];
            let Ok(handle) = u64::try_from(next.node) else {
                return Err(invalid("successor handle does not fit u64"));
            };
            let Some(&target) = record_by_handle.get(&handle) else {
                continue;
            };
            *records[target]
                .has_predecessor
                .get_mut(next.offset)
                .ok_or_else(|| invalid("successor offset is outside local record"))? = true;
        }
    }
    let occurrence_limit = records.iter().try_fold(0_usize, |total, record| {
        total
            .checked_add(record.successors.len())
            .ok_or_else(|| invalid("local occurrence count overflow"))
    })?;
    let mut starts = Vec::new();
    let mut reference_matches = 0_u64;
    for record in &records {
        for offset in 0..record.successors.len() {
            if record.has_predecessor[offset] {
                continue;
            }
            let position = Pos::new(u64_to_usize(record.handle)?, offset);
            let mut current = Some(position);
            let mut traversal = Vec::new();
            let mut is_reference = false;
            while let Some(value) = current {
                let handle = usize_to_u64(value.node)?;
                is_reference |= (handle, usize_to_u64(value.offset)?) == payload.reference_position;
                traversal.push(handle);
                let current_record = record_by_handle
                    .get(&handle)
                    .and_then(|index| records.get(*index))
                    .ok_or_else(|| invalid("local traversal left selected records"))?;
                let next = *current_record
                    .successors
                    .get(value.offset)
                    .ok_or_else(|| invalid("local traversal offset is out of bounds"))?;
                let next_handle = usize_to_u64(next.node)?;
                current = (next.node != gbz::ENDMARKER
                    && record_by_handle.contains_key(&next_handle))
                .then_some(next);
                if traversal.len() > occurrence_limit {
                    return Err(invalid("cyclic local GBWT traversal"));
                }
            }
            if is_reference {
                reference_matches = reference_matches
                    .checked_add(1)
                    .ok_or_else(|| invalid("reference occurrence count overflow"))?;
            }
            starts.push(TraversalStart {
                position,
                canonical: path_is_canonical(&traversal),
                traversal,
                is_reference,
            });
        }
    }
    if reference_matches != 1 {
        return Err(invalid(format!(
            "expected one reference occurrence, found {reference_matches}"
        )));
    }
    Ok(starts)
}

fn direct_groups(
    payload: &RecordRegionalPayload,
    starts: &[TraversalStart],
    located: &[SourceLocatedPosition],
    path_count: u64,
    identity: &TileMembershipIdentity<'_>,
) -> io::Result<Vec<TraversalMembershipGroup>> {
    if starts.len() != located.len() {
        return Err(invalid(
            "GBWT locate result count differs from traversal starts",
        ));
    }
    let mut occurrences = BTreeMap::<Vec<u64>, Vec<SourceLocatedPosition>>::new();
    for (start, identity) in starts.iter().zip(located.iter().copied()) {
        if identity.path_id >= path_count {
            return Err(invalid(
                "GBWT locate result refers outside the path catalog",
            ));
        }
        if !start.is_reference && start.canonical {
            occurrences
                .entry(start.traversal.clone())
                .or_default()
                .push(identity);
        }
    }
    let anonymous = payload
        .reconstruct_traversals()?
        .anonymous
        .into_iter()
        .map(|group| (group.handles, group.weight))
        .collect::<BTreeMap<_, _>>();
    if anonymous.len() != occurrences.len() {
        return Err(invalid("named and anonymous traversal group counts differ"));
    }
    let mut groups = Vec::with_capacity(occurrences.len());
    for (traversal, identities) in occurrences {
        let occurrence_weight = usize_to_u64(identities.len())?;
        if anonymous.get(&traversal) != Some(&occurrence_weight) {
            return Err(invalid(
                "membership multiplicity sum differs from anonymous weight",
            ));
        }
        let mut multiplicities = BTreeMap::<(u64, bool), u64>::new();
        let mut unique_paths = BTreeSet::new();
        for identity in identities {
            unique_paths.insert(identity.path_id);
            let multiplicity = multiplicities
                .entry((identity.path_id, identity.reversed))
                .or_default();
            *multiplicity = multiplicity
                .checked_add(1)
                .ok_or_else(|| invalid("path membership multiplicity overflow"))?;
        }
        let memberships = multiplicities
            .into_iter()
            .map(
                |((path_id, reversed_relative_to_group), multiplicity)| PathMembership {
                    path_id,
                    multiplicity,
                    reversed_relative_to_group,
                },
            )
            .collect::<Vec<_>>();
        groups.push(TraversalMembershipGroup {
            traversal_digest: traversal_membership_digest(
                identity.sample,
                identity.contig,
                identity.core_start,
                identity.core_end,
                identity.regional_payload_integrity,
                &traversal,
            ),
            occurrence_weight,
            unique_path_count: usize_to_u64(unique_paths.len())?,
            memberships,
        });
    }
    groups.sort_by_key(|group| group.traversal_digest);
    Ok(groups)
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

fn read_catalog(path: &Path) -> io::Result<Vec<PathCatalogRecord>> {
    let mut declared_paths = None;
    let mut records = Vec::new();
    for (line_index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let value: Value = serde_json::from_str(&line?)?;
        match value.get("type").and_then(Value::as_str) {
            Some("metadata") => {
                declared_paths = value.get("paths").and_then(Value::as_u64);
            }
            Some("path") => {
                let record: InputCatalogRecord = serde_json::from_value(value)?;
                let expected = u64::try_from(records.len())
                    .map_err(|_| invalid("path catalog count does not fit u64"))?;
                if record.canonical_path_id != expected {
                    return Err(invalid(format!(
                        "path catalog record {} is not contiguous at line {}",
                        record.canonical_path_id,
                        line_index + 1
                    )));
                }
                records.push(PathCatalogRecord {
                    path_id: record.canonical_path_id,
                    canonical_name: record.raw_name,
                    sample: record.sample,
                    contig: record.contig,
                    haplotype: record.haplotype,
                    fragment: record.fragment,
                    sense: sense_code(&record.path_sense)?,
                });
            }
            _ => {
                return Err(invalid(format!(
                    "unknown catalog row at line {}",
                    line_index + 1
                )));
            }
        }
    }
    let count = u64::try_from(records.len()).map_err(|_| invalid("path catalog is too large"))?;
    if count == 0 || declared_paths != Some(count) {
        return Err(invalid("path catalog metadata count mismatch"));
    }
    Ok(records)
}

fn tile_is_encoded(
    manifests: &[ReferenceManifest],
    sample: &str,
    contig: &str,
    tile: &InputTile,
) -> bool {
    manifests.iter().any(|manifest| {
        manifest.sample == sample
            && manifest.contig == contig
            && tile.core_start >= manifest.start
            && tile.core_end <= manifest.end
            && tile.core_start < tile.core_end
    })
}

fn append_membership_directories(
    archive: &mut File,
    buckets: &[Vec<Vec<PathMembershipDirectoryEntry>>],
    metrics: &mut PathMembershipBuildMetrics,
) -> io::Result<Vec<PathMembershipManifest>> {
    let mut manifests = Vec::with_capacity(buckets.len());
    for (manifest_index, manifest_buckets) in buckets.iter().enumerate() {
        let first_page_offset = archive.seek(SeekFrom::End(0))?;
        let mut entry_count = 0_u64;
        for entries in manifest_buckets {
            let page = encode_path_membership_directory_page(entries)?;
            archive.write_all(&page)?;
            checked_add(
                &mut metrics.page_encoded_bytes,
                PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES as u64,
                "membership encoded page bytes",
            )?;
            checked_add(
                &mut metrics.page_decoded_bytes,
                PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES as u64,
                "membership decoded page bytes",
            )?;
            checked_add(
                &mut metrics.directory_pages,
                1,
                "membership directory pages",
            )?;
            checked_add(
                &mut entry_count,
                usize_to_u64(entries.len())?,
                "membership directory entries",
            )?;
        }
        if entry_count == 0 {
            return Err(invalid("path-membership manifest contains no tiles"));
        }
        manifests.push(PathMembershipManifest {
            manifest_index: u32::try_from(manifest_index)
                .map_err(|_| invalid("membership manifest index does not fit u32"))?,
            first_page_offset,
            page_count: usize_to_u64(manifest_buckets.len())?,
            entry_count,
        });
    }
    Ok(manifests)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn write_path_membership_extension(
    archive: &mut File,
    manifests: &[ReferenceManifest],
    bucket_entries: &[Vec<Vec<ArchiveEntry>>],
    summary_path: &Path,
    catalog_path: &Path,
    data_offset: u64,
    identity_source_sha256: [u8; 32],
) -> io::Result<(ExtensionEntry, PathMembershipBuildMetrics)> {
    let summary: InputSummary = serde_json::from_slice(&std::fs::read(summary_path)?)?;
    if summary.manifest.sample.is_empty()
        || summary.manifest.contig.is_empty()
        || summary.tiles.is_empty()
    {
        return Err(invalid("path-membership summary is empty"));
    }
    let catalog = read_catalog(catalog_path)?;
    let path_count =
        u64::try_from(catalog.len()).map_err(|_| invalid("path catalog is too large"))?;
    let mut metrics = PathMembershipBuildMetrics {
        catalog_records: path_count,
        ..PathMembershipBuildMetrics::default()
    };

    let records_per_page = usize::try_from(PATH_CATALOG_RECORDS_PER_PAGE)
        .map_err(|_| invalid("catalog page size does not fit usize"))?;
    let mut catalog_pages = Vec::new();
    for records in catalog.chunks(records_per_page) {
        let raw = encode_path_catalog_page(records)?;
        let storage = append_page(archive, ChunkCodec::Zstd3, &raw, data_offset)?;
        checked_add(
            &mut metrics.page_encoded_bytes,
            storage.encoded_len,
            "membership encoded page bytes",
        )?;
        checked_add(
            &mut metrics.page_decoded_bytes,
            u64::try_from(raw.len())
                .map_err(|_| invalid("catalog page length does not fit u64"))?,
            "membership decoded page bytes",
        )?;
        catalog_pages.push(PathCatalogPageDescriptor {
            first_path_id: records[0].path_id,
            record_count: u64::try_from(records.len())
                .map_err(|_| invalid("catalog page count does not fit u64"))?,
            storage,
        });
        metrics.catalog_pages += 1;
    }

    if manifests.len() != bucket_entries.len() {
        return Err(invalid("membership manifest and directory counts differ"));
    }
    let mut graph_integrity = HashMap::new();
    for (manifest, graph_buckets) in manifests.iter().zip(bucket_entries) {
        for entry in graph_buckets.iter().flatten() {
            if graph_integrity
                .insert(
                    (
                        manifest.sample.clone(),
                        manifest.contig.clone(),
                        entry.start,
                        entry.end,
                    ),
                    entry.integrity,
                )
                .is_some()
            {
                return Err(invalid("duplicate graph tile identity"));
            }
        }
    }
    let mut tiles = HashMap::new();
    for tile in summary.tiles {
        if !tile_is_encoded(
            manifests,
            &summary.manifest.sample,
            &summary.manifest.contig,
            &tile,
        ) {
            return Err(invalid(format!(
                "membership tile {}#{}:{}-{} is outside the encoded manifests",
                summary.manifest.sample, summary.manifest.contig, tile.core_start, tile.core_end
            )));
        }
        let tile_key = (
            summary.manifest.sample.clone(),
            summary.manifest.contig.clone(),
            tile.core_start,
            tile.core_end,
        );
        let regional_payload_integrity = graph_integrity
            .get(&tile_key)
            .ok_or_else(|| invalid("prepared membership tile has no graph payload integrity"))?;
        let mut groups = Vec::with_capacity(tile.groups.len());
        for group in tile.groups {
            if group.traversal.is_empty() {
                return Err(invalid("path-membership group has an empty traversal"));
            }
            let memberships = group
                .memberships
                .into_iter()
                .map(|item| PathMembership {
                    path_id: item.path_id,
                    multiplicity: item.multiplicity,
                    reversed_relative_to_group: item.reversed_relative_to_group,
                })
                .collect::<Vec<_>>();
            if memberships.iter().any(|item| item.path_id >= path_count) {
                return Err(invalid("path-membership group refers outside the catalog"));
            }
            groups.push(TraversalMembershipGroup {
                traversal_digest: traversal_membership_digest(
                    &summary.manifest.sample,
                    &summary.manifest.contig,
                    tile.core_start,
                    tile.core_end,
                    regional_payload_integrity,
                    &group.traversal,
                ),
                occurrence_weight: group.occurrence_weight,
                unique_path_count: group.unique_path_count,
                memberships,
            });
        }
        groups.sort_by_key(|group| group.traversal_digest);
        record_group_metrics(&mut metrics, &groups)?;
        let raw = encode_tile_membership_page(
            tile.core_start,
            tile.core_end,
            regional_payload_integrity,
            &groups,
        )?;
        let storage = append_page(archive, ChunkCodec::Zstd3, &raw, data_offset)?;
        checked_add(
            &mut metrics.page_encoded_bytes,
            storage.encoded_len,
            "membership encoded page bytes",
        )?;
        checked_add(
            &mut metrics.page_decoded_bytes,
            u64::try_from(raw.len())
                .map_err(|_| invalid("membership page length does not fit u64"))?,
            "membership decoded page bytes",
        )?;
        if tiles
            .insert(
                tile_key,
                PathMembershipDirectoryEntry {
                    group_count: u64::try_from(groups.len())
                        .map_err(|_| invalid("group count does not fit u64"))?,
                    storage,
                },
            )
            .is_some()
        {
            return Err(invalid("duplicate path-membership tile"));
        }
        metrics.tile_pages += 1;
    }
    let mut membership_buckets = Vec::with_capacity(bucket_entries.len());
    for (manifest, graph_buckets) in manifests.iter().zip(bucket_entries) {
        let mut manifest_buckets = Vec::with_capacity(graph_buckets.len());
        for graph_entries in graph_buckets {
            let mut membership_entries = Vec::with_capacity(graph_entries.len());
            for entry in graph_entries {
                membership_entries.push(
                    tiles
                        .remove(&(
                            manifest.sample.clone(),
                            manifest.contig.clone(),
                            entry.start,
                            entry.end,
                        ))
                        .ok_or_else(|| {
                            invalid(format!(
                                "prepared membership is missing tile {}#{}:{}-{}",
                                manifest.sample, manifest.contig, entry.start, entry.end
                            ))
                        })?,
                );
            }
            manifest_buckets.push(membership_entries);
        }
        membership_buckets.push(manifest_buckets);
    }
    if !tiles.is_empty() {
        return Err(invalid(
            "prepared membership contains tiles absent from the graph directory",
        ));
    }
    let membership_manifests =
        append_membership_directories(archive, &membership_buckets, &mut metrics)?;
    let descriptor = encode_path_membership_descriptor(&PathMembershipDescriptor {
        path_count,
        records_per_catalog_page: PATH_CATALOG_RECORDS_PER_PAGE,
        identity_source: PathIdentitySource::PreparedAuthenticatedOracleV1,
        identity_source_sha256,
        group_count: metrics.groups,
        occurrence_total: metrics.occurrence_total,
        unique_path_total: metrics.unique_path_total,
        delta_group_count: metrics.delta_groups,
        run_group_count: metrics.run_groups,
        catalog_pages,
        manifests: membership_manifests,
    })?;
    metrics.descriptor_encoded_bytes = u64::try_from(descriptor.len())
        .map_err(|_| invalid("membership descriptor length does not fit u64"))?;
    let mut descriptor_metrics = FeatureBuildMetrics::default();
    let entry = append_descriptor(
        archive,
        PATH_MEMBERSHIP_TYPE_ID,
        &descriptor,
        &mut descriptor_metrics,
        data_offset,
    )?;
    Ok((entry, metrics))
}

/// Generates the catalog and tile membership pages directly from the active
/// disk-backed GBZ source. Regional payloads are decoded one at a time and all
/// LF work is bounded by `max_lf_steps`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn write_direct_path_membership_extension(
    archive: &mut File,
    archive_path: &Path,
    source: &dyn PangenomeSource,
    manifests: &[ReferenceManifest],
    bucket_entries: &[Vec<Vec<ArchiveEntry>>],
    max_lf_steps: usize,
    data_offset: u64,
    identity_source_sha256: [u8; 32],
) -> io::Result<(ExtensionEntry, PathMembershipBuildMetrics)> {
    let source_catalog = source.path_catalog()?.ok_or_else(|| {
        invalid("active source does not provide a complete named path catalog for membership")
    })?;
    if source_catalog.is_empty() {
        return Err(invalid("source path catalog is empty"));
    }
    if source_catalog
        .iter()
        .enumerate()
        .any(|(index, record)| record.path_id != index as u64)
    {
        return Err(invalid("source path catalog IDs are not contiguous"));
    }
    let path_count = usize_to_u64(source_catalog.len())?;
    let mut metrics = PathMembershipBuildMetrics {
        catalog_records: path_count,
        ..PathMembershipBuildMetrics::default()
    };
    let records_per_page = usize::try_from(PATH_CATALOG_RECORDS_PER_PAGE)
        .map_err(|_| invalid("catalog records per page does not fit usize"))?;
    let mut catalog_pages = Vec::new();
    for source_records in source_catalog.chunks(records_per_page) {
        let records = source_records
            .iter()
            .map(|record| PathCatalogRecord {
                path_id: record.path_id,
                canonical_name: record.canonical_name.clone(),
                sample: record.sample.clone(),
                contig: record.contig.clone(),
                haplotype: record.haplotype,
                fragment: record.fragment,
                sense: record.sense,
            })
            .collect::<Vec<_>>();
        let raw = encode_path_catalog_page(&records)?;
        let storage = append_page(archive, ChunkCodec::Zstd3, &raw, data_offset)?;
        checked_add(
            &mut metrics.page_encoded_bytes,
            storage.encoded_len,
            "membership encoded page bytes",
        )?;
        checked_add(
            &mut metrics.page_decoded_bytes,
            usize_to_u64(raw.len())?,
            "membership decoded page bytes",
        )?;
        catalog_pages.push(PathCatalogPageDescriptor {
            first_path_id: records[0].path_id,
            record_count: usize_to_u64(records.len())?,
            storage,
        });
        checked_add(&mut metrics.catalog_pages, 1, "catalog page count")?;
    }

    if manifests.len() != bucket_entries.len() {
        return Err(invalid("membership manifest and directory counts differ"));
    }
    let mut payload_reader = File::open(archive_path)?;
    let mut membership_buckets = Vec::with_capacity(bucket_entries.len());
    for (manifest, buckets) in manifests.iter().zip(bucket_entries) {
        let mut manifest_buckets = Vec::with_capacity(buckets.len());
        for bucket in buckets {
            let mut membership_entries = Vec::with_capacity(bucket.len());
            for entry in bucket {
                let encoded_len = u64_to_usize(entry.compressed_len)?;
                let mut encoded = vec![0_u8; encoded_len];
                payload_reader.seek(SeekFrom::Start(entry.offset))?;
                payload_reader.read_exact(&mut encoded)?;
                let raw = decompress(entry.codec, &encoded, entry.uncompressed_len)?;
                drop(encoded);
                let payload = RecordRegionalPayload::decode(&raw)?;
                drop(raw);
                let starts = traversal_starts(&payload)?;
                let positions = starts
                    .iter()
                    .map(|start| start.position)
                    .collect::<Vec<_>>();
                let locate_started = Instant::now();
                let located = source
                    .locate_positions(&positions, max_lf_steps)?
                    .ok_or_else(|| invalid("active source does not provide bounded GBWT locate"))?;
                metrics.locate_wall_ms += locate_started.elapsed().as_secs_f64() * 1_000.0;
                checked_add(
                    &mut metrics.located_positions,
                    usize_to_u64(located.len())?,
                    "located position count",
                )?;
                metrics.maximum_lf_steps = metrics
                    .maximum_lf_steps
                    .max(located.iter().map(|item| item.lf_steps).max().unwrap_or(0));
                let groups = direct_groups(
                    &payload,
                    &starts,
                    &located,
                    path_count,
                    &TileMembershipIdentity {
                        sample: &manifest.sample,
                        contig: &manifest.contig,
                        core_start: entry.start,
                        core_end: entry.end,
                        regional_payload_integrity: &entry.integrity,
                    },
                )?;
                drop(payload);
                record_group_metrics(&mut metrics, &groups)?;
                let raw_page =
                    encode_tile_membership_page(entry.start, entry.end, &entry.integrity, &groups)?;
                let storage = append_page(archive, ChunkCodec::Zstd3, &raw_page, data_offset)?;
                checked_add(
                    &mut metrics.page_encoded_bytes,
                    storage.encoded_len,
                    "membership encoded page bytes",
                )?;
                checked_add(
                    &mut metrics.page_decoded_bytes,
                    usize_to_u64(raw_page.len())?,
                    "membership decoded page bytes",
                )?;
                membership_entries.push(PathMembershipDirectoryEntry {
                    group_count: usize_to_u64(groups.len())?,
                    storage,
                });
                checked_add(&mut metrics.tile_pages, 1, "membership tile page count")?;
            }
            manifest_buckets.push(membership_entries);
        }
        membership_buckets.push(manifest_buckets);
    }
    if metrics.tile_pages == 0 {
        return Err(invalid("direct path membership found no encoded tiles"));
    }
    let membership_manifests =
        append_membership_directories(archive, &membership_buckets, &mut metrics)?;
    let descriptor = encode_path_membership_descriptor(&PathMembershipDescriptor {
        path_count,
        records_per_catalog_page: PATH_CATALOG_RECORDS_PER_PAGE,
        identity_source: PathIdentitySource::EmbeddedGbwtDaBoundedLfV1,
        identity_source_sha256,
        group_count: metrics.groups,
        occurrence_total: metrics.occurrence_total,
        unique_path_total: metrics.unique_path_total,
        delta_group_count: metrics.delta_groups,
        run_group_count: metrics.run_groups,
        catalog_pages,
        manifests: membership_manifests,
    })?;
    metrics.descriptor_encoded_bytes = usize_to_u64(descriptor.len())?;
    let mut descriptor_metrics = FeatureBuildMetrics::default();
    let entry = append_descriptor(
        archive,
        PATH_MEMBERSHIP_TYPE_ID,
        &descriptor,
        &mut descriptor_metrics,
        data_offset,
    )?;
    Ok((entry, metrics))
}

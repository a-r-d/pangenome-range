//! Construction-only support for the document-array samples embedded in GBWT.
//!
//! The public `gbz` crate deliberately treats this option as opaque. The
//! encoder needs only the four Simple-SDS structures used by `gbwt::DASamples`;
//! keeping the parser here avoids adding a forked GBWT implementation to either
//! the archive format or the decoder.

use simple_sds::bit_vector::BitVector;
use simple_sds::bit_vector::rank_support::RankSupport;
use simple_sds::int_vector::IntVector;
use simple_sds::ops::{Access, BitVec, PredSucc, Select, Vector};
use simple_sds::serialize::Serialize;
use simple_sds::sparse_vector::SparseVector;
use std::io::{self, Read, Write};

const CACHE_MAGIC: &[u8; 8] = b"PNGRDA01";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Parsed GBWT document-array samples.
pub(crate) struct GbwtLocate {
    sampled_records: BitVector,
    record_rank: RankSupport,
    bwt_ranges: SparseVector,
    sampled_offsets: SparseVector,
    sequence_ids: IntVector,
}

impl GbwtLocate {
    /// Loads the outer optional-vector length and parses its payload directly
    /// from the input stream. This never retains a second raw copy of the DA.
    pub(crate) fn load<R: Read>(reader: &mut R) -> io::Result<Option<Self>> {
        let elements = usize::load(reader)?;
        if elements == 0 {
            return Ok(None);
        }
        let bytes = elements
            .checked_mul(std::mem::size_of::<u64>())
            .ok_or_else(|| invalid("GBWT document-array byte length overflow"))?;
        let mut payload = reader.take(
            u64::try_from(bytes)
                .map_err(|_| invalid("GBWT document-array byte length does not fit u64"))?,
        );
        let sampled_records = BitVector::load(&mut payload)?;
        let record_rank = RankSupport::new(&sampled_records);
        let bwt_ranges = SparseVector::load(&mut payload)?;
        let sampled_offsets = SparseVector::load(&mut payload)?;
        let sequence_ids = IntVector::load(&mut payload)?;
        if payload.limit() != 0 {
            return Err(invalid("trailing words in GBWT document-array option"));
        }
        let result = Self {
            sampled_records,
            record_rank,
            bwt_ranges,
            sampled_offsets,
            sequence_ids,
        };
        result.validate()?;
        Ok(Some(result))
    }

    pub(crate) fn try_locate(&self, record: usize, offset: usize) -> Option<usize> {
        if record >= self.sampled_records.len() || !self.sampled_records.get(record) {
            return None;
        }
        let record_rank = self.record_rank.rank(&self.sampled_records, record);
        let record_start = self.bwt_ranges.select(record_rank)?;
        let global_offset = record_start.checked_add(offset)?;
        let (sample_rank, sample_offset) =
            self.sampled_offsets.predecessor(global_offset).next()?;
        (sample_offset == global_offset)
            .then(|| usize::try_from(self.sequence_ids.get(sample_rank)).ok())
            .flatten()
    }

    /// Serializes the parsed locate support without the GBWT optional-vector wrapper.
    pub(crate) fn save<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(CACHE_MAGIC)?;
        self.sampled_records.serialize(writer)?;
        self.bwt_ranges.serialize(writer)?;
        self.sampled_offsets.serialize(writer)?;
        self.sequence_ids.serialize(writer)
    }

    /// Loads locate support from the project-owned persistent-cache representation.
    pub(crate) fn load_cache<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0_u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != CACHE_MAGIC {
            return Err(invalid("invalid cached GBWT document-array header"));
        }
        let sampled_records = BitVector::load(reader)?;
        let record_rank = RankSupport::new(&sampled_records);
        let result = Self {
            sampled_records,
            record_rank,
            bwt_ranges: SparseVector::load(reader)?,
            sampled_offsets: SparseVector::load(reader)?,
            sequence_ids: IntVector::load(reader)?,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> io::Result<()> {
        if self.sampled_records.count_ones() != self.bwt_ranges.count_ones() {
            return Err(invalid(
                "GBWT DA sampled-record and BWT-range counts differ",
            ));
        }
        if self.bwt_ranges.len() != self.sampled_offsets.len() {
            return Err(invalid(
                "GBWT DA BWT-range and sampled-offset lengths differ",
            ));
        }
        if self.sampled_offsets.count_ones() != self.sequence_ids.len() {
            return Err(invalid(
                "GBWT DA sampled-offset and sequence-ID counts differ",
            ));
        }
        Ok(())
    }
}

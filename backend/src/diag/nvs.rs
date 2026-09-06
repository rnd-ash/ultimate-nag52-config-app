//! Parser for an ESP-IDF NVS partition image.
//!
//! Layout is decoded explicitly from little-endian bytes rather than by reinterpreting the
//! raw image as a `#[repr(packed)]` struct, so the code does not depend on host endianness
//! or on the compiler's field layout.

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub enum DataType {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    BlobIndex {
        size: u32,
        chunk_count: u8,
        chunk_start: u8,
        rsv: u16,
    },
    VariableData {
        size: u16,
        rsv: u16,
        crc32: u32,
    },
}

impl DataType {
    pub fn from_entry(ty: u8, raw: u64) -> Self {
        match ty {
            0x01 => Self::U8((raw & 0xFF) as u8),
            0x11 => Self::I8((raw & 0xFF) as i8),
            0x02 => Self::U16((raw & 0xFFFF) as u16),
            0x12 => Self::I16((raw & 0xFFFF) as i16),
            0x04 => Self::U32((raw & 0xFFFFFFFF) as u32),
            0x14 => Self::I32((raw & 0xFFFFFFFF) as i32),
            0x18 => Self::I64(raw as i64),
            0x42 | 0x21 => Self::VariableData {
                size: ((raw) & 0xFFFF) as u16,
                rsv: ((raw >> 16) & 0xFFFF) as u16,
                crc32: ((raw >> 32) & 0xFFFFFFFF) as u32,
            },
            0x48 => Self::BlobIndex {
                size: ((raw) & 0xFFFFFFFF) as u32,
                chunk_count: ((raw >> 32) & 0xFF) as u8,
                chunk_start: ((raw >> 40) & 0xFF) as u8,
                rsv: ((raw >> 48) & 0xFFFF) as u16,
            },
            _ => Self::U64(raw),
        }
    }
}

/// Size of one NVS entry on flash, in bytes.
pub const NVS_ENTRY_SIZE: usize = 32;
/// Number of entries in one NVS page.
pub const NVS_ENTRIES_PER_PAGE: usize = 126;
/// Size of the fixed page header (state, seqnr, reserved, crc, bitmap), in bytes.
pub const NVS_PAGE_HEADER_SIZE: usize = 64;
/// Total size of one NVS page on flash, in bytes.
pub const NVS_PAGE_SIZE: usize = NVS_PAGE_HEADER_SIZE + (NVS_ENTRIES_PER_PAGE * NVS_ENTRY_SIZE);

/// State of one entry slot, as encoded in the page's 2-bits-per-entry bitmap.
///
/// Discriminants match ESP-IDF's `Page::EntryState` (`nvs_page.hpp`): the bitmap stores
/// `0b11` for an untouched slot, because erased flash reads as all-ones.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum EntryState {
    /// `0b00` - the entry was written and later erased.
    Erased = 0,
    /// `0b01` - a write started but did not complete (e.g. reset mid-write).
    Invalid = 1,
    /// `0b10` - a live entry.
    Written = 2,
    /// `0b11` - never used; erased flash reads as all-ones.
    Empty = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvsParseError {
    /// The image does not contain a whole number of pages.
    TruncatedPage { page_index: usize, remaining: usize },
}

impl std::fmt::Display for NvsParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedPage { page_index, remaining } => write!(
                f,
                "NVS image truncated at page {page_index}: {remaining} bytes left, need {NVS_PAGE_SIZE}"
            ),
        }
    }
}

impl std::error::Error for NvsParseError {}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct NvsEntry {
    pub ns: u8,
    pub ty: u8,
    pub span: u8,
    pub chunk_index: u8,
    pub crc: u32,
    pub key: [u8; 16],
    pub data: u64,
}

impl NvsEntry {
    pub fn get_key(&self) -> String {
        String::from_utf8_lossy(&self.key).to_string()
    }

    /// Decodes one entry. `raw` must be exactly [`NVS_ENTRY_SIZE`] bytes.
    fn parse(raw: &[u8; NVS_ENTRY_SIZE]) -> Self {
        let mut key = [0u8; 16];
        key.copy_from_slice(&raw[8..24]);
        Self {
            ns: raw[0],
            ty: raw[1],
            span: raw[2],
            chunk_index: raw[3],
            crc: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            key,
            data: u64::from_le_bytes([
                raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31],
            ]),
        }
    }

    /// The decoded value carried by this entry.
    pub fn data_type(&self) -> DataType {
        DataType::from_entry(self.ty, self.data)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct NvsPage {
    pub state: u32,
    pub seqnr: u32,
    pub unused: [u32; 5],
    pub crc: u32,
    pub bitmap: [u8; 32],
    pub entries: [NvsEntry; NVS_ENTRIES_PER_PAGE],
}

impl NvsPage {
    /// Decodes one page. `raw` must be exactly [`NVS_PAGE_SIZE`] bytes.
    fn parse(raw: &[u8]) -> Self {
        debug_assert_eq!(raw.len(), NVS_PAGE_SIZE);
        let rd_u32 = |off: usize| {
            u32::from_le_bytes([raw[off], raw[off + 1], raw[off + 2], raw[off + 3]])
        };

        let mut unused = [0u32; 5];
        for (i, slot) in unused.iter_mut().enumerate() {
            *slot = rd_u32(8 + (i * 4));
        }

        let mut bitmap = [0u8; 32];
        bitmap.copy_from_slice(&raw[32..64]);

        let mut entries = [NvsEntry {
            ns: 0,
            ty: 0,
            span: 0,
            chunk_index: 0,
            crc: 0,
            key: [0u8; 16],
            data: 0,
        }; NVS_ENTRIES_PER_PAGE];
        for (i, entry) in entries.iter_mut().enumerate() {
            let start = NVS_PAGE_HEADER_SIZE + (i * NVS_ENTRY_SIZE);
            let mut buf = [0u8; NVS_ENTRY_SIZE];
            buf.copy_from_slice(&raw[start..start + NVS_ENTRY_SIZE]);
            *entry = NvsEntry::parse(&buf);
        }

        Self {
            state: rd_u32(0),
            seqnr: rd_u32(4),
            unused,
            crc: rd_u32(28),
            bitmap,
            entries,
        }
    }

    /// State of entry slot `idx`, read from the page's 2-bits-per-entry bitmap.
    pub fn entry_state(&self, idx: usize) -> Option<EntryState> {
        if idx >= NVS_ENTRIES_PER_PAGE {
            return None;
        }
        Some(match (self.bitmap[idx / 4] >> ((idx % 4) * 2)) & 0x03 {
            0 => EntryState::Erased,
            1 => EntryState::Invalid,
            2 => EntryState::Written,
            _ => EntryState::Empty,
        })
    }

    /// Reassembles the payload of a variable-length entry at `idx`.
    ///
    /// `span` counts the header entry itself, so the payload lives in the following
    /// `span - 1` entries. Returns `None` if the entry does not carry variable data or if
    /// its span runs past the end of the page.
    pub fn read_blob(&self, idx: usize) -> Option<Vec<u8>> {
        let entry = self.entries.get(idx)?;
        let DataType::VariableData { size, .. } = entry.data_type() else {
            return None;
        };
        let span = entry.span as usize;
        if span < 1 || idx + span > NVS_ENTRIES_PER_PAGE {
            return None;
        }
        let mut blob: Vec<u8> = Vec::with_capacity((span - 1) * NVS_ENTRY_SIZE);
        for child in 1..span {
            let e = &self.entries[idx + child];
            blob.extend_from_slice(&e.ns.to_le_bytes());
            blob.extend_from_slice(&e.ty.to_le_bytes());
            blob.extend_from_slice(&e.span.to_le_bytes());
            blob.extend_from_slice(&e.chunk_index.to_le_bytes());
            blob.extend_from_slice(&e.crc.to_le_bytes());
            blob.extend_from_slice(&e.key);
            blob.extend_from_slice(&e.data.to_le_bytes());
        }
        if blob.len() < size as usize {
            return None;
        }
        blob.truncate(size as usize);
        Some(blob)
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct NvsPartition {
    pub pages: Vec<NvsPage>,
}

impl NvsPartition {
    /// Parses a whole NVS partition image.
    ///
    /// Returns an error rather than panicking when the image does not contain a whole
    /// number of [`NVS_PAGE_SIZE`] pages.
    pub fn parse(data: &[u8]) -> Result<Self, NvsParseError> {
        let mut pages = Vec::with_capacity(data.len() / NVS_PAGE_SIZE);
        let mut offset = 0usize;
        while offset < data.len() {
            let remaining = data.len() - offset;
            if remaining < NVS_PAGE_SIZE {
                return Err(NvsParseError::TruncatedPage {
                    page_index: pages.len(),
                    remaining,
                });
            }
            pages.push(NvsPage::parse(&data[offset..offset + NVS_PAGE_SIZE]));
            offset += NVS_PAGE_SIZE;
        }
        Ok(Self { pages })
    }

    /// Every written entry in the image, as `(page index, entry index)`.
    pub fn written_entries(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (page_idx, page) in self.pages.iter().enumerate() {
            let mut i = 0usize;
            while i < NVS_ENTRIES_PER_PAGE {
                if page.entry_state(i) == Some(EntryState::Written) {
                    out.push((page_idx, i));
                    // `span` is untrusted image data: a zero span would not advance the
                    // cursor and would spin forever.
                    let span = page.entries[i].span as usize;
                    i += span.max(1);
                } else {
                    i += 1;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_matches_esp_idf_layout() {
        assert_eq!(NVS_PAGE_SIZE, 4096);
    }

    #[test]
    fn truncated_image_is_an_error_not_a_panic() {
        let data = vec![0u8; NVS_PAGE_SIZE + 10];
        let err = NvsPartition::parse(&data).unwrap_err();
        assert_eq!(
            err,
            NvsParseError::TruncatedPage {
                page_index: 1,
                remaining: 10
            }
        );
    }

    #[test]
    fn empty_image_parses_to_no_pages() {
        assert_eq!(NvsPartition::parse(&[]).unwrap().pages.len(), 0);
    }

    #[test]
    fn each_page_is_consumed_exactly_once() {
        // Two pages, distinguishable by their `seqnr` field at offset 4.
        let mut data = vec![0u8; NVS_PAGE_SIZE * 2];
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[NVS_PAGE_SIZE + 4..NVS_PAGE_SIZE + 8].copy_from_slice(&2u32.to_le_bytes());
        let part = NvsPartition::parse(&data).unwrap();
        assert_eq!(part.pages.len(), 2);
        assert_eq!(part.pages[0].seqnr, 1);
        assert_eq!(part.pages[1].seqnr, 2);
    }

    #[test]
    fn entry_fields_decode_little_endian() {
        let mut data = vec![0u8; NVS_PAGE_SIZE];
        let e = NVS_PAGE_HEADER_SIZE;
        data[e] = 0xAA; // ns
        data[e + 1] = 0x02; // ty = u16
        data[e + 2] = 0x01; // span
        data[e + 3] = 0x00; // chunk index
        data[e + 4..e + 8].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data[e + 8..e + 11].copy_from_slice(b"key");
        data[e + 24..e + 32].copy_from_slice(&0x1234u64.to_le_bytes());

        let part = NvsPartition::parse(&data).unwrap();
        let entry = part.pages[0].entries[0];
        assert_eq!(entry.ns, 0xAA);
        assert_eq!(entry.crc, 0xDEADBEEF);
        assert_eq!(entry.data, 0x1234);
        assert!(entry.get_key().starts_with("key"));
        assert_eq!(entry.data_type(), DataType::U16(0x1234));
    }

    #[test]
    fn zero_span_cannot_hang_the_scan() {
        let mut data = vec![0u8; NVS_PAGE_SIZE];
        // Mark every entry slot WRITTEN (0b10) and leave all spans at zero.
        for b in data[32..64].iter_mut() {
            *b = 0b10_10_10_10;
        }
        let part = NvsPartition::parse(&data).unwrap();
        assert_eq!(part.written_entries().len(), NVS_ENTRIES_PER_PAGE);
    }

    /// Pins the bitmap encoding to ESP-IDF's `Page::EntryState` (`nvs_page.hpp`).
    /// Getting this backwards makes `written_entries()` return erased/invalid slots and
    /// miss every live entry, which is silent rather than loud.
    #[test]
    fn entry_state_matches_esp_idf_encoding() {
        let mut data = vec![0u8; NVS_PAGE_SIZE];
        // Slots 0..4 share bitmap byte 0: states 0b00, 0b01, 0b10, 0b11 (low bits first).
        data[32] = 0b11_10_01_00;
        let part = NvsPartition::parse(&data).unwrap();
        let page = &part.pages[0];
        assert_eq!(page.entry_state(0), Some(EntryState::Erased));
        assert_eq!(page.entry_state(1), Some(EntryState::Invalid));
        assert_eq!(page.entry_state(2), Some(EntryState::Written));
        assert_eq!(page.entry_state(3), Some(EntryState::Empty));
        // Only the WRITTEN slot counts as live.
        assert_eq!(page.entry_state(4), Some(EntryState::Erased));
        assert_eq!(part.written_entries(), vec![(0, 2)]);
    }

    #[test]
    fn entry_state_is_bounds_checked() {
        let data = vec![0u8; NVS_PAGE_SIZE];
        let part = NvsPartition::parse(&data).unwrap();
        assert_eq!(part.pages[0].entry_state(NVS_ENTRIES_PER_PAGE), None);
    }

    #[test]
    fn blob_span_past_end_of_page_is_rejected() {
        let mut data = vec![0u8; NVS_PAGE_SIZE];
        let last = NVS_PAGE_HEADER_SIZE + ((NVS_ENTRIES_PER_PAGE - 1) * NVS_ENTRY_SIZE);
        data[last + 1] = 0x42; // variable data
        data[last + 2] = 0x10; // span of 16, way past the end
        let part = NvsPartition::parse(&data).unwrap();
        assert_eq!(part.pages[0].read_blob(NVS_ENTRIES_PER_PAGE - 1), None);
    }
}

use std::{fs::File, io::Write, mem::size_of};
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

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
#[repr(C, packed)]
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
}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
#[repr(C, packed)]
pub struct NvsPage {
    pub state: u32,
    pub seqnr: u32,
    pub unused: [u32; 5],
    pub crc: u32,
    pub bitmap: [u8; 32],
    pub entries: [NvsEntry; 126],
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct NvsPartition {
    pub pages: Vec<NvsPage>,
}

impl NvsPartition {
    pub fn new(data: Vec<u8>) -> Self {
        let mut f = File::create("EEPROM.bin").unwrap();
        f.write_all(&data).unwrap();
        let mut offset = 0;
        let mut page_n = 0;
        println!("{:02X?}", &data[0..20]);
        let mut pages = Vec::new();
        while offset < data.len() {
            println!("Reading page {}", page_n);
            let blk: [u8; size_of::<NvsPage>()] = data[offset..offset + size_of::<NvsPage>()]
                .try_into()
                .unwrap();
            let page: NvsPage = unsafe { std::mem::transmute(blk) };
            println!("page state {:02X}", unsafe { page.state });
            pages.push(page);
            offset += size_of::<NvsPage>();
            let mut i = 0;
            while i < 126 {
                let bm = (page.bitmap[i / 4] >> ((i % 4) * 2)) & 0x03;
                if bm == 2 {
                    println!(
                        "Key {} in page {}. Ty {:02X}, Span {} entries. ChkIdx: {}",
                        page.entries[i].get_key(),
                        page_n,
                        page.entries[i].ty,
                        page.entries[i].span,
                        page.entries[i].chunk_index
                    );
                    let ty = DataType::from_entry(page.entries[i].ty, page.entries[i].data);

                    if let DataType::VariableData { size, rsv, crc32 } = ty {
                        let mut blob: Vec<u8> = vec![];
                        for child in 1..=page.entries[i].span as usize {
                            let re_interp: [u8; size_of::<NvsEntry>()] =
                                unsafe { std::mem::transmute(page.entries[i + child]) };
                            blob.extend_from_slice(&re_interp);
                        }
                        blob.resize(size as usize, 0);
                        println!("Blob data: {:02X?}", blob);
                    }
                    i += page.entries[i].span as usize;
                } else {
                    println!("BM {:02X?}", page.entries[i]);
                    i += 1;
                }
            }
            offset += size_of::<NvsPage>();
            page_n += 1;
        }
        Self { pages }
    }
}

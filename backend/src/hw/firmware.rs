use std::{fs::File, io::Read};

use packed_struct::{prelude::PackedStruct, PackedStructSlice};
use static_assertions::assert_eq_size;

const HEADER_SIZE: usize = 256;
const HEADER_MAGIC: [u8; 4] = [0x32, 0x54, 0xCD, 0xAB];
assert_eq_size!([u8; HEADER_SIZE], FirmwareHeader);

pub type FirwmareLoadResult<T> = std::result::Result<T, FirmwareLoadError>;

#[derive(Debug, Clone, Copy, PackedStruct)]
pub struct FirmwareHeader {
    #[packed_field(endian = "lsb")]
    magic: u32,
    #[packed_field(endian = "lsb")]
    secure_version: u32,
    #[packed_field(endian = "lsb")]
    _reserved1: [u32; 2],
    version: [u8; 32],
    project_name: [u8; 32],
    time: [u8; 16],
    date: [u8; 16],
    idf_ver: [u8; 32],
    app_elf_sha: [u8; 32],
    #[packed_field(endian = "lsb")]
    _reserved2: [u32; 20],
}

impl FirmwareHeader {
    pub fn get_version(&self) -> String {
        String::from_utf8(self.version.to_vec()).unwrap_or("UNKNOWN".into()).trim_matches(char::from(0)).to_string()
    }
    pub fn get_idf_version(&self) -> String {
        String::from_utf8(self.idf_ver.to_vec()).unwrap_or("UNKNOWN".into()).trim_matches(char::from(0)).to_string()
    }
    pub fn get_date(&self) -> String {
        String::from_utf8(self.date.to_vec()).unwrap_or("UNKNOWN".into()).trim_matches(char::from(0)).to_string()
    }
    pub fn get_time(&self) -> String {
        String::from_utf8(self.time.to_vec()).unwrap_or("UNKNOWN".into()).trim_matches(char::from(0)).to_string()
    }
    pub fn get_fw_name(&self) -> String {
        String::from_utf8(self.project_name.to_vec()).unwrap_or("UNKNOWN".into()).trim_matches(char::from(0)).to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Firmware {
    pub raw: Vec<u8>,
    pub header: FirmwareHeader,
}

#[derive(Debug)]
pub enum FirmwareLoadError {
    NotValid(String),
    IoError(std::io::Error),
}

impl From<std::io::Error> for FirmwareLoadError {
    fn from(f: std::io::Error) -> Self {
        Self::IoError(f)
    }
}

pub fn load_binary(path: String) -> FirwmareLoadResult<Firmware> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    // Todo find a nicer way to do this!
    let mut header_start_idx = 0;
    loop {
        let tmp = &buf[header_start_idx..];
        if tmp.len() < HEADER_MAGIC.len() || header_start_idx > 50 {
            return Err(FirmwareLoadError::NotValid(
                "Could not find header magic".into(),
            ));
        }
        if tmp[..HEADER_MAGIC.len()] == HEADER_MAGIC {
            break; // Found!
        }
        header_start_idx += 1;
    }

    if buf[header_start_idx..].len() < HEADER_SIZE {
        return Err(FirmwareLoadError::NotValid(
            "Could not find header magic".into(),
        ));
    }
    // Ok, read the header
    let header: FirmwareHeader = FirmwareHeader::unpack_from_slice(&buf[header_start_idx..header_start_idx+HEADER_SIZE]).unwrap();
    Ok(Firmware { raw: buf, header })
}

use std::io::{Write, Read};

use flate2::{write::ZlibEncoder, Compression, bufread::ZlibDecoder};
use packed_struct::prelude::PackedStruct;

use super::settings::ModuleSettingsData;

const MODULE_SETING_FLASH_MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsFlashReadError {
    InvalidMagic,
    UncompressFailure,
    InvalidContentSize
}

#[derive(PackedStruct, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleSettingsFlashHeader {
    // Reserved for ESP
    pub magic: [u8; 4],
    #[packed_field(endian = "lsb")]
    pub key_magic: u32, // Hash of all EEPROM keys
    #[packed_field(endian = "lsb")]
    pub length_compressed: u32
}

const HEADER_SIZE: usize = std::mem::size_of::<ModuleSettingsFlashHeader>();

impl ModuleSettingsFlashHeader {
    pub fn new_from_yml_content(f_str: &str) -> Option<(Self, Vec<u8>)> {
        let settings: ModuleSettingsData = serde_yaml::from_str(f_str).ok()?;
        // Key magic calc algo
        let mut cs: u32 = 0;
        for key in settings.settings.iter().map(|x| &x.eeprom_key) {
            if let Some(k) = key {
                let b = k.as_str().as_bytes();
                for (idx, byte) in b.iter().enumerate() {
                    cs = cs.wrapping_add(idx as u32);
                    cs = cs.wrapping_add(*byte as u32);
                }
            }
        }


        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(f_str.as_bytes()).unwrap();
        let bytes = encoder.finish().unwrap();
        Some((Self {
            magic: MODULE_SETING_FLASH_MAGIC,
            key_magic: cs,
            length_compressed: bytes.len() as u32
        },
        bytes))
    }

    pub fn read_header_from_buffer(flash_read: &[u8]) -> Result<Self, MsFlashReadError> {
        if flash_read.len() <= HEADER_SIZE {
            Err(MsFlashReadError::InvalidContentSize)
        } else if flash_read[0..4] != MODULE_SETING_FLASH_MAGIC {
            Err(MsFlashReadError::InvalidMagic)
        } else {
            Ok(ModuleSettingsFlashHeader::unpack(&flash_read[0..HEADER_SIZE].try_into().unwrap()).unwrap())
        }
    }

    pub fn from_flash_bytes_to_yml_bytes(flash_read: &[u8]) -> Result<(Self, Vec<u8>), MsFlashReadError> {
        if flash_read.len() <= HEADER_SIZE {
            Err(MsFlashReadError::InvalidContentSize)
        } else if flash_read[0..4] != MODULE_SETING_FLASH_MAGIC {
            Err(MsFlashReadError::InvalidMagic)
        } else {
            let header = ModuleSettingsFlashHeader::unpack(&flash_read[0..HEADER_SIZE].try_into().unwrap()).unwrap();
            let data = &flash_read[HEADER_SIZE..];
            if data.len() < header.length_compressed as usize {
                return Err(MsFlashReadError::InvalidContentSize);
            }
            let mut decoder = ZlibDecoder::new(&data[..header.length_compressed as usize]);
            let mut ret = Vec::new();
            match decoder.read_to_end(&mut ret) {
                Ok(_) => {
                    Ok((header, ret))
                },
                Err(e) => {
                    println!("{e:?}");
                    Err(MsFlashReadError::UncompressFailure)
                },
            }
        }
    }

    pub fn merge_to_tx_data(&self, compressed: &[u8]) -> Vec<u8> {
        let mut tx = self.pack().unwrap().to_vec();
        tx.extend_from_slice(compressed);
        tx
    }

}
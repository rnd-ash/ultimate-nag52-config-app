use std::io::{Write, Read};

use flate2::{write::ZlibEncoder, Compression, read::GzDecoder, bufread::ZlibDecoder};
use packed_struct::prelude::PackedStruct;

const MODULE_SETING_FLASH_MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsFlashReadError {
    InvalidMagic,
    UncompressFailure,
    InvalidContentSize
}

#[derive(PackedStruct, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleSettingsFlashHeader {
    pub magic: [u8; 4],
    #[packed_field(endian = "lsb")]
    pub length_compressed: u32
}

const HEADER_SIZE: usize = std::mem::size_of::<ModuleSettingsFlashHeader>();

impl ModuleSettingsFlashHeader {
    pub fn new_from_yml_content(f_bytes: &[u8]) -> (Self, Vec<u8>) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(f_bytes).unwrap();
        let bytes = encoder.finish().unwrap();
        (Self {
            magic: MODULE_SETING_FLASH_MAGIC,
            length_compressed: bytes.len() as u32
        },
        bytes)
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

#[cfg(test)]
pub mod ModuleSettingsCompressTest {
    use super::ModuleSettingsFlashHeader;

    #[test]
    pub fn verify_read_back() {
        // A big file
        let contents_to_compress = include_bytes!("../../../../ultimate-nag52-fw/MODULE_SETTINGS.yml");
        let (header, compressed_data) = ModuleSettingsFlashHeader::new_from_yml_content(contents_to_compress);
        // Resize compressed data to be a LOT larger (To emulating reading an entire partition on ESP)
        let mut tx_to_flash = header.merge_to_tx_data(&compressed_data);
        println!("Raw size: {}, compressed size (Inc header): {}", contents_to_compress.len(), tx_to_flash.len());
        tx_to_flash.resize(0x19000, 0x00);
        let (header, raw) = ModuleSettingsFlashHeader::from_flash_bytes_to_yml_bytes(&tx_to_flash).unwrap();
        assert_eq!(raw, contents_to_compress);
    }
}
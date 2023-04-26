use ecu_diagnostics::{
    kwp2000::{self, KwpSessionTypeByte},
    DiagError, DiagServerResult,
};
use packed_struct::{prelude::PackedStruct, PackedStructSlice};

use crate::hw::firmware::FirmwareHeader;

use super::Nag52Diag;

#[derive(PackedStruct, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartitionInfo {
    #[packed_field(endian = "lsb")]
    pub address: u32,
    #[packed_field(endian = "lsb")]
    pub size: u32,
}

pub const OTA_FORMAT: u8 = 0xF0;

impl Nag52Diag {
    pub fn get_total_flash_size(&self) -> PartitionInfo {
        PartitionInfo {
            address: 0x0,
            size: 0x400000,
        }
    }

    pub fn get_coredump_flash_info(&mut self) -> DiagServerResult<PartitionInfo> {
        self.with_kwp(|server| {
            server.kwp_read_custom_local_identifier(0x29).map(|res| {
                PartitionInfo::unpack_from_slice(&res).map_err(|_| DiagError::InvalidResponseLength)
            })?
        })
    }

    pub fn get_running_partition_flash_info(&mut self) -> DiagServerResult<PartitionInfo> {
        self.with_kwp(|server| {
            server.kwp_read_custom_local_identifier(0x2A).map(|res| {
                PartitionInfo::unpack_from_slice(&res).map_err(|_| DiagError::InvalidResponseLength)
            })?
        })
    }

    pub fn get_next_ota_partition_flash_info(&mut self) -> DiagServerResult<PartitionInfo> {
        self.with_kwp(|server| {
            server.kwp_read_custom_local_identifier(0x2B).map(|res| {
                PartitionInfo::unpack_from_slice(&res).map_err(|_| DiagError::InvalidResponseLength)
            })?
        })
    }

    pub fn get_running_fw_info(&mut self) -> DiagServerResult<FirmwareHeader> {
        self.with_kwp(|server| {
            server.kwp_read_custom_local_identifier(0x28).map(|res| {
                println!("{:02X?}", res);
                FirmwareHeader::unpack_from_slice(&res)
                    .map_err(|_| DiagError::InvalidResponseLength)
            })?
        })
    }

    pub fn begin_ota(&mut self, image_len: u32) -> DiagServerResult<(u32, u16)> {
        let part_info_next = self.get_next_ota_partition_flash_info()?;
        let res = self.with_kwp(|server| {
            server.kwp_set_session(KwpSessionTypeByte::Standard(
                kwp2000::KwpSessionType::Reprogramming,
            ))?;
            let x = part_info_next.address;
            let mut req: Vec<u8> =
                vec![0x34, (x >> 16) as u8, (x >> 8) as u8, (x) as u8, OTA_FORMAT];
            req.push((image_len >> 16) as u8);
            req.push((image_len >> 8) as u8);
            req.push((image_len) as u8);
            let resp = server.send_byte_array_with_response(&req)?;
            let bs = (resp[1] as u16) << 8 | resp[2] as u16;
            Ok((part_info_next.address, bs))
        });
        res
    }

    pub fn begin_download(&mut self, partition_info: &PartitionInfo) -> DiagServerResult<u16> {
        let res = self.with_kwp(|server| {
            server.kwp_set_session(KwpSessionTypeByte::Standard(
                kwp2000::KwpSessionType::Reprogramming,
            ))?;
            let x = partition_info.address;
            let mut req: Vec<u8> = vec![0x35, (x >> 16) as u8, (x >> 8) as u8, (x) as u8, 0x00];
            req.push((partition_info.size >> 16) as u8);
            req.push((partition_info.size >> 8) as u8);
            req.push((partition_info.size) as u8);
            let resp = server.send_byte_array_with_response(&req)?;
            let bs = (resp[1] as u16) << 8 | resp[2] as u16;
            Ok(bs)
        });
        res
    }

    pub fn transfer_data(&mut self, blk_id: u8, data: &[u8]) -> DiagServerResult<()> {
        self.with_kwp(|server| {
            let mut req = vec![0x36, blk_id];
            req.extend_from_slice(data);
            server.send_byte_array_with_response(&req).map(|_| ())
        })
    }

    pub fn read_data(&mut self, blk_id: u8) -> DiagServerResult<Vec<u8>> {
        self.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[0x36, blk_id])
                .map(|x| x[2..].to_vec())
        })
    }

    pub fn end_ota(&mut self, reboot: bool) -> DiagServerResult<()> {
        self.with_kwp(|server| {
            server.send_byte_array_with_response(&[0x37])?;
            let status = server.send_byte_array_with_response(&[0x31, 0xE1])?;
            if status[2] == 0x00 {
                eprintln!("ECU Flash check OK! Rebooting");
                if reboot {
                    server.kwp_reset_ecu(kwp2000::ResetType::PowerOnReset)?;
                }
                Ok(())
            } else {
                eprintln!("ECU Flash check failed :(");
                Err(DiagError::NotSupported)
            }
        })
    }
}

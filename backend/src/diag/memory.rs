use ecu_diagnostics::DiagServerResult;

use super::Nag52Diag;

#[derive(Debug, Clone, Copy)]
pub enum MemoryRegion {
    Sram0,
    Sram1,
    Sram2,
    Psram,
    EgsCalibration
}

impl MemoryRegion {
    pub fn start_addr(&self) -> u32 {
        match self {
            MemoryRegion::Sram0 => 0x00_FFFF,
            MemoryRegion::Sram1 => 0x03_FFFF,
            MemoryRegion::Sram2 => 0x05_1FFF,
            MemoryRegion::Psram => 0x10_0000,
            MemoryRegion::EgsCalibration => 0x80_0000,
        }
    }

    pub fn end_addr(&self) -> u32 {
        match self {
            MemoryRegion::Sram0 => 0x02_FFFF,
            MemoryRegion::Sram1 => 0x04_FFFF,
            MemoryRegion::Sram2 => 0x07_1FFF,
            MemoryRegion::Psram => 0x4F_FFFF,
            MemoryRegion::EgsCalibration => 0x87_D000,
        }
    }
}


/// Checks that `[start + pos, start + pos + len]` stays inside the region.
///
/// The additions are done with `checked_add`: plain `u32` arithmetic wraps in release
/// builds, which would let a large `pos` slip past the guard and send an arbitrary
/// address to the TCU (and panic in debug builds instead).
fn end_of_access(region: MemoryRegion, pos: u32, len: u32) -> Option<u32> {
    let end = region.start_addr().checked_add(pos)?.checked_add(len)?;
    if end > region.end_addr() { None } else { Some(end) }
}

impl Nag52Diag {
    pub fn read_memory(&self, region: MemoryRegion, pos: u32, len: u8) -> DiagServerResult<Vec<u8>> {
        if end_of_access(region, pos, len as u32).is_none() {
            Err(ecu_diagnostics::DiagError::ParameterInvalid)
        } else {
            // Valid address
            let start_address = region.start_addr() + pos;
            let req = vec![
                0x23,
                ((start_address >> 16) & 0xFF) as u8,
                ((start_address >>  8) & 0xFF) as u8,
                ((start_address >>  0) & 0xFF) as u8,
                len
            ];
            self.with_kwp(|kwp| {
                kwp.send_byte_array_with_response(&req, None)
            })
        }
    }

    /// Max of 251 bytes at a time!
    pub fn write_memory(&self, region: MemoryRegion, pos: u32, data: &[u8]) -> DiagServerResult<Vec<u8>> {
        if end_of_access(region, pos, data.len() as u32).is_none() {
            Err(ecu_diagnostics::DiagError::ParameterInvalid)
        } else if data.len() > 251 { // Too much data
            Err(ecu_diagnostics::DiagError::ParameterInvalid)
        } else {
            // Valid address
            let start_address = region.start_addr() + pos;
            let mut req = vec![
                0x3D,
                ((start_address >> 16) & 0xFF) as u8,
                ((start_address >>  8) & 0xFF) as u8,
                ((start_address >>  0) & 0xFF) as u8,
                data.len() as u8
            ];
            req.extend_from_slice(data);
            self.with_kwp(|kwp| {
                kwp.send_byte_array_with_response(&req, None)
            })
        }
    }
}
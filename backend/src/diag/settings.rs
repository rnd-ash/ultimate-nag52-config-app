use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone)] 
pub enum SettingsType {
    Bool(bool),
    F32(f32),
    U16(u16),
    I16(i16),
    U8(u8),
    Enum { value: u8, mapping: EnumMap },
    Struct { raw: Vec<u8>, s: SettingsData } 
}

impl From<SettingsType> for Vec<u8> {
    fn from(value: SettingsType) -> Self {
        match value {
            SettingsType::Bool(b) => vec![b as u8],
            SettingsType::F32(v) => v.to_le_bytes().to_vec(),
            SettingsType::U16(v) => v.to_le_bytes().to_vec(),
            SettingsType::I16(v) => v.to_le_bytes().to_vec(),
            SettingsType::U8(v) => vec![v],
            SettingsType::Enum { value, mapping: _ } => vec![value],
            SettingsType::Struct { raw, s: _ } => raw,
        }
    }
}

/// Why a coding string could not be decoded against its YAML description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsDecodeError {
    /// The declared offset/length runs past the end of the coding string.
    OutOfBounds {
        name: String,
        offset: usize,
        size: usize,
        available: usize,
    },
    /// The declared width does not match the data type (e.g. a 3-byte `float`).
    BadWidth {
        name: String,
        data_type: String,
        size: usize,
    },
    /// The YAML names a type that is neither a primitive, an enum nor a known struct.
    UnknownDataType { name: String, data_type: String },
}

impl std::fmt::Display for SettingsDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds { name, offset, size, available } => write!(
                f,
                "Setting '{name}' spans bytes {offset}..{} but the coding string is only {available} bytes",
                offset + size
            ),
            Self::BadWidth { name, data_type, size } => write!(
                f,
                "Setting '{name}' is declared as '{data_type}' but with a length of {size} bytes"
            ),
            Self::UnknownDataType { name, data_type } => write!(
                f,
                "No settings variable data type found for '{data_type}' (setting '{name}')"
            ),
        }
    }
}

impl std::error::Error for SettingsDecodeError {}

#[derive(Debug, Clone, Deserialize)]
pub struct EnumDesc {
    #[serde(rename="Name")]
    pub name: String,
    #[serde(rename="Desc")]
    pub desc: String
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnumMap {
    #[serde(rename="Name")]
    pub name: String,
    #[serde(rename="Mappings")]
    pub mappings: HashMap<u8, EnumDesc>
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsVariable {
    #[serde(rename="Name")]
    pub name: String,
    #[serde(rename="Description")]
    pub description: Option<String>,
    #[serde(rename="Unit")]
    pub unit: Option<String>,
    #[serde(rename="DataType")]
    pub data_type: String,
    #[serde(rename="OffsetBytes")]
    pub offset_bytes: usize,
    #[serde(rename="LengthBytes")]
    pub size_bytes: usize
}

impl SettingsVariable {
    /// Decodes this variable out of a coding string.
    ///
    /// Both the coding string (read from the TCU) and the offset/length/type metadata
    /// (read from `MODULE_SETTINGS.yml`) are external inputs, so a mismatch between them
    /// is reported as an error instead of panicking in the UI thread.
    pub fn to_settings_type(
        &self,
        raw: &[u8],
        enums: &[EnumMap],
        structs: &[SettingsData],
    ) -> Result<SettingsType, SettingsDecodeError> {
        let end = self.offset_bytes.saturating_add(self.size_bytes);
        if self.size_bytes == 0 || end > raw.len() {
            return Err(SettingsDecodeError::OutOfBounds {
                name: self.name.clone(),
                offset: self.offset_bytes,
                size: self.size_bytes,
                available: raw.len(),
            });
        }
        let bytes = &raw[self.offset_bytes..end];
        let bad_width = || SettingsDecodeError::BadWidth {
            name: self.name.clone(),
            data_type: self.data_type.clone(),
            size: self.size_bytes,
        };
        Ok(match self.data_type.as_str() {
            "bool" => SettingsType::Bool(bytes[0] != 0),
            "float" => SettingsType::F32(f32::from_le_bytes(
                bytes.try_into().map_err(|_| bad_width())?,
            )),
            "uint16_t" => SettingsType::U16(u16::from_le_bytes(
                bytes.try_into().map_err(|_| bad_width())?,
            )),
            "int16_t" => SettingsType::I16(i16::from_le_bytes(
                bytes.try_into().map_err(|_| bad_width())?,
            )),
            "uint8_t" => SettingsType::U8(bytes[0]),
            name => {
                for e in enums {
                    if e.name == name {
                        return Ok(SettingsType::Enum { value: bytes[0], mapping: e.clone() })
                    }
                }

                for s in structs {
                    if s.name == name {
                        return Ok(SettingsType::Struct { raw: bytes.to_vec(), s: s.clone() })
                    }
                }

                // Check structs and enums
                return Err(SettingsDecodeError::UnknownDataType {
                    name: self.name.clone(),
                    data_type: self.data_type.clone(),
                });
            }
        })
    }

    /// Writes an edited value back into the coding string.
    ///
    /// Returns an error rather than panicking when the encoded value does not match the
    /// declared width, or when the declared span falls outside the coding string.
    pub fn insert_back_into_coding_string(
        &self,
        setting_ty: SettingsType,
        raw_coding_string: &mut [u8],
    ) -> Result<(), SettingsDecodeError> {
        let raw: Vec<u8> = setting_ty.into();
        let end = self.offset_bytes.saturating_add(self.size_bytes);
        if end > raw_coding_string.len() {
            return Err(SettingsDecodeError::OutOfBounds {
                name: self.name.clone(),
                offset: self.offset_bytes,
                size: self.size_bytes,
                available: raw_coding_string.len(),
            });
        }
        if raw.len() != self.size_bytes {
            return Err(SettingsDecodeError::BadWidth {
                name: self.name.clone(),
                data_type: self.data_type.clone(),
                size: self.size_bytes,
            });
        }
        raw_coding_string[self.offset_bytes..end].copy_from_slice(&raw);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsData {
    #[serde(rename="Name")]
    pub name: String,
    #[serde(rename="Description")]
    pub description: Option<String>,
    #[serde(rename="SCN_ID")]
    pub scn_id: Option<u8>,
    #[serde(rename="EEPROM_KEY")]
    pub eeprom_key: Option<String>,
    #[serde(rename="Params")]
    pub params: Vec<SettingsVariable>
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleSettingsData {
    #[serde(rename="Enums")]
    pub enums: Vec<EnumMap>,
    #[serde(rename="IStructs")]
    pub internal_structures: Vec<SettingsData>,
    #[serde(rename="Settings")]
    pub settings: Vec<SettingsData>
}
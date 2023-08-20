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

impl Into<Vec<u8>> for SettingsType {
    fn into(self) -> Vec<u8> {
        match self {
            SettingsType::Bool(b) => vec![b as u8],
            SettingsType::F32(v) => v.to_le_bytes().to_vec(),
            SettingsType::U16(v) => v.to_le_bytes().to_vec(),
            SettingsType::I16(v) => v.to_le_bytes().to_vec(),
            SettingsType::U8(v) => vec![v],
            SettingsType::Enum { value, mapping: _ } => vec![value],
            SettingsType::Struct { raw, s: _ } => {
                raw.clone()
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnumMap {
    #[serde(rename="Name")]
    pub name: String,
    #[serde(rename="Mappings")]
    pub mappings: HashMap<u8, String>
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
    pub fn to_settings_type(&self, raw: &[u8], enums: &[EnumMap], structs: &[SettingsData]) -> SettingsType {
        let bytes = &raw[self.offset_bytes..self.offset_bytes + self.size_bytes];
        match self.data_type.as_str() {
            "bool" => SettingsType::Bool(bytes[0] != 0),
            "float" => SettingsType::F32(f32::from_le_bytes(bytes.try_into().unwrap())),
            "uint16_t" => SettingsType::U16(u16::from_le_bytes(bytes.try_into().unwrap())),
            "int16_t" => SettingsType::I16(i16::from_le_bytes(bytes.try_into().unwrap())),
            "uint8_t" => SettingsType::U8(bytes[0]),
            name => {
                for e in enums {
                    if e.name == name {
                        return SettingsType::Enum { value: bytes[0], mapping: e.clone() }
                    }
                }

                for s in structs {
                    if s.name == name {
                        return SettingsType::Struct { raw: bytes.to_vec(), s: s.clone() }
                    }
                }

                // Check structs and enums
                panic!("No settings variable data type found for '{}'", self.data_type);
            }
        }
    }

    pub fn insert_back_into_coding_string(&self, setting_ty: SettingsType, raw_coding_string: &mut [u8]) {
        let raw: Vec<u8> = setting_ty.into();
        raw_coding_string[self.offset_bytes..self.offset_bytes+self.size_bytes].copy_from_slice(&raw)
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
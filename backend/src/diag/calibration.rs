use std::collections::{BTreeMap, BTreeSet};
use packed_struct::derive::PackedStruct;
use serde_big_array::BigArray;
use serde_derive::{Serialize, Deserialize};

#[derive(PackedStruct, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
#[repr(C)]
#[packed_struct(endian = "lsb")]
pub struct EgsShiftMapConfiguration {
    // Momentum maps
    pub momentum_1_2_x: [u8; 3],
    pub momentum_2_3_x: [u8; 3],
    pub momentum_3_4_x: [u8; 3],
    pub momentum_4_5_x: [u8; 3],
    pub momentum_1_2_y: [u8; 2],
    pub momentum_2_3_y: [u8; 2],
    pub momentum_3_4_y: [u8; 2],
    pub momentum_4_5_y: [u8; 2],
    pub momentum_1_2_z: [u8; 6],
    pub momentum_2_3_z: [u8; 6],
    pub momentum_3_4_z: [u8; 6],
    pub momentum_4_5_z: [u8; 6],

    pub momentum_2_1_x: [u8; 6],
    pub momentum_3_2_x: [u8; 6],
    pub momentum_4_3_x: [u8; 6],
    pub momentum_5_4_x: [u8; 6],
    pub momentum_2_1_y: [u8; 10],
    pub momentum_3_2_y: [u8; 10],
    pub momentum_4_3_y: [u8; 10],
    pub momentum_5_4_y: [u8; 10],
    #[serde(with = "BigArray")]
    pub momentum_2_1_z: [u8; 60],
    #[serde(with = "BigArray")]
    pub momentum_3_2_z: [u8; 60],
    #[serde(with = "BigArray")]
    pub momentum_4_3_z: [u8; 60],
    #[serde(with = "BigArray")]
    pub momentum_5_4_z: [u8; 60],

    pub trq_adder_1_2_x: [u8; 6],
    pub trq_adder_2_3_x: [u8; 6],
    pub trq_adder_3_4_x: [u8; 6],
    pub trq_adder_4_5_x: [u8; 6],
    pub trq_adder_1_2_y: [u8; 8],
    pub trq_adder_2_3_y: [u8; 8],
    pub trq_adder_3_4_y: [u8; 8],
    pub trq_adder_4_5_y: [u8; 8],
    #[serde(with = "BigArray")]
    pub trq_adder_1_2_z: [u8; 48],
    #[serde(with = "BigArray")]
    pub trq_adder_2_3_z: [u8; 48],
    #[serde(with = "BigArray")]
    pub trq_adder_3_4_z: [u8; 48],
    #[serde(with = "BigArray")]
    pub trq_adder_4_5_z: [u8; 48],

    pub torque_adder_2_1_x: [u8; 3],
    pub torque_adder_3_2_x: [u8; 3],
    pub torque_adder_4_3_x: [u8; 3],
    pub torque_adder_5_4_x: [u8; 3],
    pub torque_adder_2_1_y: [u8; 4],
    pub torque_adder_3_2_y: [u8; 4],
    pub torque_adder_4_3_y: [u8; 4],
    pub torque_adder_5_4_y: [u8; 4],
    #[serde(with = "BigArray")]
    pub torque_adder_2_1_z: [u8; 12],
    #[serde(with = "BigArray")]
    pub torque_adder_3_2_z: [u8; 12],
    #[serde(with = "BigArray")]
    pub torque_adder_4_3_z: [u8; 12],
    #[serde(with = "BigArray")]
    pub torque_adder_5_4_z: [u8; 12],
}

#[derive(PackedStruct, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[repr(C)]
#[packed_struct(endian = "lsb")]
pub struct EgsHydraulicConfiguration {
    pub multiplier_1: u16,
    pub multiplier_other: u16,
    pub lp_reg_pressure: u16,
    pub spc_overlap_circuit_factor: [u16; 8],
    pub mpc_overlap_circuit_factor: [u16; 8],
    pub spring_overlap_pressure: [i16; 8],
    pub shift_reg_pressure: u16,
    pub spc_gain_factor_shift: [u16; 8],
    pub min_mpc_pressure: u16,
    pub unk1: u8,
    pub unk2: u8,
    pub unk3: u16,
    pub unk4: u16,
    pub unk5: u16,
    pub shift_pressure_addr_percent: u16,
    pub inlet_pressure_offset: u16,
    pub inlet_pressure_input_min: u16,
    pub inlet_pressure_input_max: u16,
    pub inlet_pressure_output_min: u16,
    pub inlet_pressure_output_max: u16,
    pub extra_pressure_pump_speed_min: u16,
    pub extra_pressure_pump_speed_max: u16,
    pub extra_pressure_adder_r1_1: u16,
    pub extra_pressure_adder_other_gears: u16,
    pub shift_pressure_factor_percent: u16,
    pub pcs_map_x: [u16; 7],
    pub pcs_map_y: [u16; 4],
    #[serde(with = "BigArray")]
    pub pcs_map_z: [u16; 28]
}

#[derive(PackedStruct, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[repr(C)]
#[packed_struct(endian = "lsb")]
pub struct EgsMechanicalConfiguration {
    pub gb_ty: u8,
    pub ratio_table: [u16; 8],
    #[serde(alias="shift_something_unk1")]
    pub intertia_factor_table: [u16; 8],
    #[serde(with = "BigArray")]
    pub friction_map: [u16; 48],
    pub max_torque_on_clutch: [u16; 4],
    pub max_torque_off_clutch: [u16; 4],
    pub release_spring_pressure: [u16; 6],
    #[serde(alias="torque_byte_unk2")]
    pub intertia_torque: [u16; 8],
    pub strongest_loaded_clutch_idx: [u8; 8],
    pub unk3: [u16; 8],
    pub atf_density_minus_50c: u16,
    pub atf_density_drop_per_c: u16,
    pub atf_density_centrifugal_force_factor: [u16; 3]
}

#[derive(PackedStruct, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[repr(C)]
#[packed_struct(endian = "lsb")]
pub struct EgsTorqueConverterConfiguration {
    pub loss_map_x: [u16; 2],
    pub loss_map_z: [u16; 2],
    pub pump_map_x: [u16; 11],
    pub pump_map_z: [u16; 11]
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalibrationRecord<T>
where T: PartialEq + Eq + PartialOrd + Ord {
    pub name: String,
    pub data: T,
    pub valid_egs_pns: Vec<String>
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChassisConfig {
    pub gearbox: String,
    pub chassis: String,
    pub hydr_cfg: String,
    pub mech_cfg: String,
    pub tcc_cfg: String,
    pub shift_algo_cfg: String
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EgsData {
    pub pn: String,
    pub chassis: Vec<ChassisConfig>
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalibrationDatabase {
    pub egs_list: Vec<EgsData>,
    pub hydralic_calibrations: Vec<CalibrationRecord<EgsHydraulicConfiguration>>,
    pub mechanical_calibrations: Vec<CalibrationRecord<EgsMechanicalConfiguration>>,
    pub torqueconverter_calibrations: Vec<CalibrationRecord<EgsTorqueConverterConfiguration>>,
    pub shift_algo_map_calibration: Vec<CalibrationRecord<EgsShiftMapConfiguration>>
}

// On the TCU itself (At address 0x34900)
#[derive(PackedStruct, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
#[packed_struct(endian = "lsb")]
pub struct EgsStoredCalibration {
    pub magic: u32,
    pub len: u16,
    pub crc: u16,
    pub tcc_cal_name: [u8;16],
    #[packed_field(element_size_bytes="52")]
    pub tcc_cal: EgsTorqueConverterConfiguration,
    pub mech_cal_name: [u8;16],
    #[packed_field(element_size_bytes="207")]
    pub mech_cal: EgsMechanicalConfiguration,
    pub hydr_cal_name: [u8;16],
    #[packed_field(element_size_bytes="182")]
    pub hydr_cal: EgsHydraulicConfiguration,
    pub shift_algo_cal_name: [u8;16],
    #[packed_field(element_size_bytes="672")]
    pub shift_algo_cal: EgsShiftMapConfiguration
}
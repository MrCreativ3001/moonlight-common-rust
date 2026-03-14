use num::FromPrimitive;
use num_derive::FromPrimitive;

#[derive(Debug, Clone, Copy)]
pub struct NalHeader {
    pub forbidden_zero_bit: bool,
    pub nal_unit_type: NalUnitType,
    pub nuh_layer_id: u8,
    pub nuh_temporal_id_plus1: u8,
}

impl NalHeader {
    pub const SIZE: usize = 2;

    pub fn parse(header: [u8; 2]) -> Self {
        // F: 1 bit
        let forbidden_zero_bit = (header[0] & 0b1000_0000) != 0;

        // Type: 6 bits
        let nal_unit_type = (header[0] & 0b0111_1110) >> 1;

        // LayerId: 6 bits
        let nuh_layer_id = ((header[0] & 0b0000_0001) << 5) | ((header[1] & 0b1111_1000) >> 3);

        // TID: 3 bits
        let nuh_temporal_id_plus1 = header[1] & 0b0000_0111;

        Self {
            forbidden_zero_bit,
            // It's impossible for this to fail because we only have 6 bits like the enum
            #[allow(clippy::unwrap_used)]
            nal_unit_type: NalUnitType::from_u8(nal_unit_type).unwrap(),
            nuh_layer_id,
            nuh_temporal_id_plus1,
        }
    }

    #[allow(unused)]
    pub fn serialize(&self) -> [u8; 2] {
        let mut header = [0u8; 2];

        if self.forbidden_zero_bit {
            header[0] |= 0b1000_0000;
        }

        // Type: 6 bits
        header[0] |= (self.nal_unit_type as u8 & 0b0011_1111) << 1;

        // LayerId: 6 bits
        header[0] |= (self.nuh_layer_id >> 5) & 0b0000_0001;
        header[1] |= (self.nuh_layer_id & 0b0001_1111) << 3;

        // TID: 3 bits
        header[1] |= self.nuh_temporal_id_plus1 & 0b0000_0111;

        header
    }
}

/// Section 7.4.2 in HEVC/H.265 specification (Table 7-1).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
pub enum NalUnitType {
    // VCL NAL units
    TrailN = 0,
    TrailR = 1,
    TsaN = 2,
    TsaR = 3,
    StsaN = 4,
    StsaR = 5,
    RadlN = 6,
    RadlR = 7,
    RaslN = 8,
    RaslR = 9,

    RsvVclN10 = 10,
    RsvVclR11 = 11,
    RsvVclN12 = 12,
    RsvVclR13 = 13,
    RsvVclN14 = 14,
    RsvVclR15 = 15,

    BlaWLp = 16,
    BlaWRadl = 17,
    BlaNLp = 18,
    IdrWRadl = 19,
    IdrNLp = 20,
    CraNut = 21,

    RsvIrapVcl22 = 22,
    RsvIrapVcl23 = 23,

    RsvVcl24 = 24,
    RsvVcl25 = 25,
    RsvVcl26 = 26,
    RsvVcl27 = 27,
    RsvVcl28 = 28,
    RsvVcl29 = 29,
    RsvVcl30 = 30,
    RsvVcl31 = 31,

    // Non-VCL NAL units
    VpsNut = 32,
    SpsNut = 33,
    PpsNut = 34,
    AudNut = 35,
    EosNut = 36,
    EobNut = 37,
    FdNut = 38,
    PrefixSeiNut = 39,
    SuffixSeiNut = 40,

    RsvNvcl41 = 41,
    RsvNvcl42 = 42,
    RsvNvcl43 = 43,
    RsvNvcl44 = 44,
    RsvNvcl45 = 45,
    RsvNvcl46 = 46,
    RsvNvcl47 = 47,

    AggregationUnit = 48,
    FragmentationUnit = 49,
    Unspec50 = 50,
    Unspec51 = 51,
    Unspec52 = 52,
    Unspec53 = 53,
    Unspec54 = 54,
    Unspec55 = 55,
    Unspec56 = 56,
    Unspec57 = 57,
    Unspec58 = 58,
    Unspec59 = 59,
    Unspec60 = 60,
    Unspec61 = 61,
    Unspec62 = 62,
    Unspec63 = 63,
}

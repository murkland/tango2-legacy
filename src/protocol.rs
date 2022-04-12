use bincode::Options;

pub const VERSION: u8 = 0x0d;

lazy_static! {
    pub static ref BINCODE_OPTIONS: bincode::config::WithOtherIntEncoding<
        bincode::config::DefaultOptions,
        bincode::config::FixintEncoding,
    > = bincode::DefaultOptions::new().with_fixint_encoding();
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum Packet {
    Hello(Hello),
    Hola(Hola),
    Init(Init),
    Input(Input),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Hello {
    pub protocol_version: u8,
    pub game_title: String,
    pub game_crc32: u32,
    pub match_type: u16,
    pub rng_commitment: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Hola {
    pub rng_nonce: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Init {
    pub battle_number: u8,
    pub input_delay: u32,
    pub marshaled: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub battle_number: u8,
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Vec<u8>,
}

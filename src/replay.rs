use crate::mgba;
use byteorder::WriteBytesExt;
use std::io::Write;

struct Writer {
    encoder: zstd::stream::write::AutoFinishEncoder<'static, Box<dyn std::io::Write>>,
}

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x09;

impl Writer {
    pub fn new(writer: Box<dyn std::io::Write>, player_index: u8) -> std::io::Result<Self> {
        let mut encoder = zstd::Encoder::new(writer, 3)?.auto_finish();
        encoder.write(HEADER)?;
        encoder.write(&[VERSION])?;
        encoder.write(&[player_index])?;
        encoder.flush()?;
        Ok(Writer { encoder })
    }

    pub fn write_init(&mut self, player_index: u8, init: &[u8]) -> std::io::Result<()> {
        self.encoder.write_u8(player_index);
        self.encoder
            .write_u32::<byteorder::LittleEndian>(init.len() as u32)?;
        self.encoder.write(init)?;
        self.encoder.flush()?;
        Ok(())
    }

    pub fn write_state(&mut self, state: &mgba::state::State) -> std::io::Result<()> {
        self.encoder
            .write_u32::<byteorder::LittleEndian>(state.as_slice().len() as u32)?;
        self.encoder.write(state.as_slice())?;
        self.encoder.flush()?;
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_tick: u32,
        remote_tick: u32,
        p1_joyflags: u16,
        p2_joyflags: u16,
        p1_custom_state: u8,
        p2_custom_state: u8,
        p1_turn: &[u8],
        p2_turn: &[u8],
    ) -> std::io::Result<()> {
        self.encoder
            .write_u32::<byteorder::LittleEndian>(local_tick)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(remote_tick)?;
        self.encoder
            .write_u16::<byteorder::LittleEndian>(p1_joyflags)?;
        self.encoder.write_u8(p1_custom_state)?;
        self.encoder
            .write_u16::<byteorder::LittleEndian>(p2_joyflags)?;
        self.encoder.write_u8(p2_custom_state)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p1_turn.len() as u32)?;
        self.encoder.write(p1_turn)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p2_turn.len() as u32)?;
        self.encoder.write(p2_turn)?;
        self.encoder.flush()?;
        Ok(())
    }
}

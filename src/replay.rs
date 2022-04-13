use crate::input;
use crate::mgba;
use byteorder::WriteBytesExt;
use std::io::Write;

pub struct Writer {
    encoder: zstd::stream::write::AutoFinishEncoder<'static, Box<dyn std::io::Write + Send>>,
}

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x09;

impl Writer {
    pub fn new(
        writer: Box<dyn std::io::Write + Send>,
        local_player_index: u8,
    ) -> std::io::Result<Self> {
        let mut encoder = zstd::Encoder::new(writer, 3)?.auto_finish();
        encoder.write_all(HEADER)?;
        encoder.write_all(&[VERSION])?;
        encoder.write_all(&[local_player_index])?;
        encoder.flush()?;
        Ok(Writer { encoder })
    }

    pub fn write_inits(&mut self, p1_init: &[u8], p2_init: &[u8]) -> std::io::Result<()> {
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p1_init.len() as u32)?;
        self.encoder.write_all(p1_init)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p2_init.len() as u32)?;
        self.encoder.write_all(p2_init)?;
        self.encoder.flush()?;
        Ok(())
    }

    pub fn write_state(&mut self, state: &mgba::state::State) -> std::io::Result<()> {
        self.encoder
            .write_u32::<byteorder::LittleEndian>(state.as_slice().len() as u32)?;
        self.encoder.write_all(state.as_slice())?;
        self.encoder.flush()?;
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &input::Pair<input::Input>,
    ) -> std::io::Result<()> {
        let (p1, p2) = if local_player_index == 0 {
            (&ip.local, &ip.remote)
        } else {
            (&ip.remote, &ip.local)
        };
        self.encoder
            .write_u32::<byteorder::LittleEndian>(ip.local.local_tick)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(ip.local.remote_tick)?;
        self.encoder
            .write_u16::<byteorder::LittleEndian>(p1.joyflags)?;
        self.encoder.write_u8(p1.custom_screen_state)?;
        self.encoder
            .write_u16::<byteorder::LittleEndian>(p2.joyflags)?;
        self.encoder.write_u8(p2.custom_screen_state)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p1.turn.len() as u32)?;
        self.encoder.write_all(&p1.turn)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p2.turn.len() as u32)?;
        self.encoder.write_all(&p2.turn)?;
        self.encoder.flush()?;
        Ok(())
    }
}

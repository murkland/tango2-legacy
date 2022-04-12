use crate::mgba;
use prost::Message;
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

    pub fn write_state(&mut self, state: &mgba::state::State) -> std::io::Result<()> {
        self.encoder.write(state.as_slice())?;
        self.encoder.flush()?;
        Ok(())
    }
}

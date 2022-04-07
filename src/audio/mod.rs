use crate::mgba::gba;

pub struct MGBAAudioSource {
    core: std::sync::Arc<std::sync::Mutex<crate::mgba::core::Core>>,
    sample_rate: u32,
    buf: Vec<i16>,
    buf_offset: usize,
}

impl MGBAAudioSource {
    pub fn new(
        core: std::sync::Arc<std::sync::Mutex<crate::mgba::core::Core>>,
        sample_rate: u32,
    ) -> Self {
        let buf = {
            let core = core.as_ref().lock().unwrap();
            vec![0; (core.get_audio_buffer_size() * 2 * 2) as usize]
        };
        Self {
            core,
            sample_rate,
            buf,
            buf_offset: 0,
        }
    }

    fn read_new_buf(&mut self) {
        let mut core = self.core.as_ref().lock().unwrap();

        let clock_rate = core.frequency();

        let n = core.get_audio_buffer_size() as i32;

        let mut faux_clock = 1.0;
        if let Some(mut sync) = core.get_gba().get_sync() {
            sync.lock_audio();
            faux_clock = gba::audio_calculate_ratio(1.0, sync.get_fps_target(), 1.0);
        }

        {
            let mut left = core.get_audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail();
            if available > n {
                available = n;
            }
            left.read_samples(&mut self.buf, available, true);
        }

        {
            let mut right = core.get_audio_channel(1);
            right.set_rates(
                clock_rate as f64,
                self.sample_rate as f64 * faux_clock as f64,
            );
            let mut available = right.samples_avail();
            if available > n {
                available = n;
            }
            right.read_samples(&mut self.buf[1..], available, true);
        }

        if let Some(mut sync) = core.get_gba().get_sync() {
            sync.consume_audio();
        }
    }
}

impl rodio::Source for MGBAAudioSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl Iterator for MGBAAudioSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf_offset >= self.buf.len() {
            self.read_new_buf();
            self.buf_offset = 0;
        }
        let sample = (self.buf[self.buf_offset] as f32) / 32768 as f32;
        self.buf_offset += 1;
        Some(sample)
    }
}

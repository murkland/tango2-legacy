pub struct RunaheadStream {
    core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
    sample_rate: cpal::SampleRate,
    channels: u16,
}

impl RunaheadStream {
    pub fn new(core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>) -> RunaheadStream {
        Self {
            core,
            sample_rate: cpal::SampleRate(0),
            channels: 2,
        }
    }
}

impl super::Stream for RunaheadStream {
    fn fill(&mut self, buf: &mut [i16]) -> usize {
        let mut core = self.core.as_ref().lock();
        let frame_count = (buf.len() / self.channels as usize) as u64;

        let clock_rate = core.as_ref().frequency();

        let n = frame_count as i32;

        let mut buf_left = &mut buf[..];

        while !buf_left.is_empty() {
            let available = {
                let mut core = core.as_mut();
                let mut left = core.audio_channel(0);
                left.set_rates(clock_rate as f64, self.sample_rate.0 as f64);
                let mut available = left.samples_avail();
                if available > n {
                    available = n;
                }
                left.read_samples(buf_left, available, self.channels == 2);
                available
            };

            if self.channels == 2 {
                let mut core = core.as_mut();
                let mut right = core.audio_channel(1);
                right.set_rates(clock_rate as f64, self.sample_rate.0 as f64);
                right.read_samples(&mut buf_left[1..], available, self.channels == 2);
            }

            buf_left = &mut buf_left[(available * 2) as usize..];
            core.as_mut().run_frame();
        }

        buf.len()
    }

    fn set_sample_rate(&mut self, sample_rate: cpal::SampleRate) {
        self.sample_rate = sample_rate
    }

    fn set_channels(&mut self, channels: u16) {
        self.channels = channels
    }
}

pub struct TimewarpStream {
    core: std::sync::Arc<parking_lot::Mutex<mgba::core::Core>>,
    sample_rate: cpal::SampleRate,
    channels: u16,
}

impl TimewarpStream {
    pub fn new(
        core: std::sync::Arc<parking_lot::Mutex<mgba::core::Core>>,
        sample_rate: cpal::SampleRate,
        channels: u16,
    ) -> TimewarpStream {
        Self {
            core,
            sample_rate,
            channels,
        }
    }
}

impl super::Stream for TimewarpStream {
    fn fill(&self, buf: &mut [i16]) -> usize {
        let mut core = self.core.as_ref().lock();
        let frame_count = (buf.len() / self.channels as usize) as i32;

        let clock_rate = core.as_ref().frequency();

        let mut faux_clock = 1.0;
        if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
            sync.lock_audio();
            faux_clock = mgba::gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
        }

        let available = {
            let mut core = core.as_mut();
            let mut left = core.audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail();
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(buf, available, self.channels == 2);
            available
        };

        if self.channels == 2 {
            let mut core = core.as_mut();
            let mut right = core.audio_channel(1);
            right.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            right.read_samples(&mut buf[1..], available, self.channels == 2);
        }

        if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
            sync.consume_audio();
        }

        available as usize * self.channels as usize
    }
}

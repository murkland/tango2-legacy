pub struct TimewarpStream {
    core: *mut mgba::c::mCore,
    sample_rate: cpal::SampleRate,
    channels: u16,
}

unsafe impl Send for TimewarpStream {}

impl TimewarpStream {
    pub fn new(
        core: &mgba::core::Core,
        sample_rate: cpal::SampleRate,
        channels: u16,
    ) -> TimewarpStream {
        Self {
            core: unsafe { core.raw_ptr() },
            sample_rate,
            channels,
        }
    }
}

impl super::Stream for TimewarpStream {
    fn fill(&self, buf: &mut [i16]) -> usize {
        let mut core = unsafe { mgba::core::CoreMutRef::from_ptr(self.core) };
        let frame_count = (buf.len() / self.channels as usize) as i32;
        let stereo = self.channels > 1;

        let clock_rate = core.as_ref().frequency();

        let mut faux_clock = 1.0;
        if let Some(sync) = core.gba_mut().sync_mut().as_mut() {
            sync.lock_audio();
            faux_clock = mgba::gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
        }

        let available = {
            let mut left = core.audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail();
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(buf, available, stereo);
            available
        };

        if stereo {
            let mut right = core.audio_channel(1);
            right.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            right.read_samples(&mut buf[1..], available, stereo);
        }

        if let Some(sync) = core.gba_mut().sync_mut().as_mut() {
            sync.consume_audio();
        }

        available as usize * self.channels as usize
    }
}

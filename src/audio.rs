use cpal::traits::DeviceTrait;

use crate::mgba::gba;

pub fn open_mgba_audio_stream(
    core: std::sync::Arc<parking_lot::Mutex<crate::mgba::core::Core>>,
    device: &cpal::Device,
    sample_rate: cpal::SampleRate,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let frame_count = core.as_ref().lock().as_mut().audio_buffer_size();
    let mut buf = vec![0; (frame_count * 2) as usize * 4];
    device.build_output_stream(
        &cpal::StreamConfig {
            channels: 2,
            sample_rate,
            buffer_size: cpal::BufferSize::Fixed(frame_count as u32),
        },
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut core = core.as_ref().lock();

            let clock_rate = core.as_ref().frequency();

            let mut faux_clock = 1.0;
            if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
                sync.lock_audio();
                faux_clock = gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
            }

            let n = ((core.as_mut().audio_buffer_size() as f64) / (faux_clock as f64)) as i32;

            let available = {
                let mut core = core.as_mut();
                let mut left = core.audio_channel(0);
                left.set_rates(clock_rate as f64, sample_rate.0 as f64);
                let mut available = left.samples_avail();
                if available > n {
                    available = n;
                }
                left.read_samples(&mut buf, available, true);
                available
            };

            {
                let mut core = core.as_mut();
                let mut right = core.audio_channel(1);
                right.set_rates(clock_rate as f64, sample_rate.0 as f64);
                right.read_samples(&mut buf[1..], available, true);
            }

            if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
                sync.consume_audio();
            }

            for i in &mut buf[available as usize * 2..] {
                *i = 0
            }

            for (x, y) in data.iter_mut().zip(buf.iter()) {
                *x = *y as f32 / 32768.0;
            }
        },
        move |_err| {},
    )
}

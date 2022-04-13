use cpal::traits::DeviceTrait;

use crate::mgba::gba;

pub fn open_mgba_audio_stream(
    core: std::sync::Arc<parking_lot::Mutex<crate::mgba::core::Core>>,
    device: &cpal::Device,
) -> Result<cpal::Stream, anyhow::Error> {
    let frame_count = core.as_ref().lock().as_mut().audio_buffer_size();
    let mut buf = vec![0; (frame_count * 2) as usize * 4];

    let config = device
        .supported_output_configs()?
        .next()
        .ok_or(anyhow::format_err!("found no supported configs"))?
        .with_max_sample_rate()
        .config();

    let channels = config.channels;
    let sample_rate = config.sample_rate;

    Ok(device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut core = core.as_ref().lock();
            let frame_count = (data.len() / channels as usize) as u64;

            let back_buf_size = data.len() * 4;
            if back_buf_size > buf.len() {
                buf = vec![0; back_buf_size];
            }

            let clock_rate = core.as_ref().frequency();

            let mut faux_clock = 1.0;
            if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
                sync.lock_audio();
                faux_clock = gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
            }

            let n = (frame_count as f64 / faux_clock as f64) as i32;

            let available = {
                let mut core = core.as_mut();
                let mut left = core.audio_channel(0);
                left.set_rates(clock_rate as f64, sample_rate.0 as f64);
                let mut available = left.samples_avail();
                if available > n {
                    available = n;
                }
                left.read_samples(&mut buf, available, channels == 2);
                available
            };

            if channels == 2 {
                let mut core = core.as_mut();
                let mut right = core.audio_channel(1);
                right.set_rates(clock_rate as f64, sample_rate.0 as f64);
                right.read_samples(&mut buf[1..], available, channels == 2);
            }

            if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
                sync.consume_audio();
            }

            for i in &mut buf[available as usize * channels as usize..] {
                *i = 0
            }

            for (x, y) in data.iter_mut().zip(buf.iter()) {
                *x = *y as f32 / 32768.0;
            }
        },
        move |_err| {},
    )?)
}

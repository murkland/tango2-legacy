use cpal::traits::DeviceTrait;

fn fill_buf(
    buf: &mut Vec<i16>,
    n: usize,
    core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
    channels: u16,
    sample_rate: cpal::SampleRate,
) -> usize {
    let mut core = core.as_ref().lock();
    let frame_count = (n / channels as usize) as u64;

    let back_buf_size = n * 4;
    if back_buf_size > buf.len() {
        *buf = vec![0i16; back_buf_size];
    }

    let clock_rate = core.as_ref().frequency();

    let mut faux_clock = 1.0;
    if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
        sync.lock_audio();
        faux_clock = crate::gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
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
        left.read_samples(buf, available, channels == 2);
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

    available as usize * channels as usize
}

pub fn open_stream(
    core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
    device: &cpal::Device,
) -> Result<cpal::Stream, anyhow::Error> {
    // TODO: Perform smarter config selection, this can choose really low bitrate configs by default.
    let mut supported_config = None;
    while let Some(f) = device.supported_output_configs()?.next() {
        if f.max_sample_rate().0 > 44100 && f.channels() == 2 {
            supported_config = Some(f.with_sample_rate(cpal::SampleRate(44100)));
        }
    }

    let supported_config = if let Some(supported_config) = supported_config {
        supported_config
    } else {
        anyhow::bail!("no supported stream config found");
    };

    let config = supported_config.config();
    log::info!("selected audio config: {:?}", config);

    let channels = config.channels;
    let sample_rate = config.sample_rate;

    let error_callback = |err| log::error!("audio stream error: {}", err);

    Ok(match supported_config.sample_format() {
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let core = core.clone();
                    let n = fill_buf(&mut buf, data.len(), core, channels, sample_rate);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as u16 + 32768;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let core = core.clone();
                    let n = fill_buf(&mut buf, data.len(), core, channels, sample_rate);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let core = core.clone();
                    let n = fill_buf(&mut buf, data.len(), core, channels, sample_rate);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as f32 / 32768.0;
                    }
                }
            },
            error_callback,
        ),
    }?)
}

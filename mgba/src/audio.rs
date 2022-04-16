use cpal::traits::DeviceTrait;

pub mod runahead_stream;
pub mod timewarp_stream;

pub fn open_stream(
    core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
    device: &cpal::Device,
    fill_buf: impl Fn(
            &mut Vec<i16>,
            usize,
            std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
            u16,
            cpal::SampleRate,
        ) -> usize
        + Send
        + 'static,
) -> Result<cpal::Stream, anyhow::Error> {
    let mut supported_configs = device.supported_output_configs()?.collect::<Vec<_>>();
    supported_configs.sort_by(|x, y| x.max_sample_rate().cmp(&y.max_sample_rate()));
    let mut supported_config = None;
    for f in supported_configs.into_iter() {
        if f.max_sample_rate().0 >= 44100 && f.max_sample_rate().0 <= 48000 && f.channels() == 2 {
            supported_config = Some(f.with_max_sample_rate());
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

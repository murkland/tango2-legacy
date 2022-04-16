use cpal::traits::DeviceTrait;

pub mod mux_stream;
pub mod timewarp_stream;

pub trait Stream {
    fn fill(&mut self, buf: &mut [i16]) -> usize;
}

pub fn get_supported_config(device: &cpal::Device) -> anyhow::Result<cpal::SupportedStreamConfig> {
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

    Ok(supported_config)
}

pub fn open_stream(
    device: &cpal::Device,
    supported_config: &cpal::SupportedStreamConfig,
    mut stream: impl Stream + Send + 'static,
) -> Result<cpal::Stream, anyhow::Error> {
    let error_callback = |err| log::error!("audio stream error: {}", err);

    Ok(match supported_config.sample_format() {
        cpal::SampleFormat::U16 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as u16 + 32768;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::F32 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as f32 / 32768.0;
                    }
                }
            },
            error_callback,
        ),
    }?)
}

pub fn fill_buf(
    buf: &mut Vec<i16>,
    n: usize,
    core: std::sync::Arc<parking_lot::Mutex<crate::core::Core>>,
    channels: u16,
    sample_rate: cpal::SampleRate,
) -> usize {
    let mut core = core.as_ref().lock();
    let frame_count = (n / channels as usize) as u64;

    if n > buf.len() {
        *buf = vec![0i16; n];
    }

    let clock_rate = core.as_ref().frequency();

    let n = frame_count as i32;

    let mut buf_left = &mut buf[..];

    while !buf_left.is_empty() {
        let available = {
            let mut core = core.as_mut();
            let mut left = core.audio_channel(0);
            left.set_rates(clock_rate as f64, sample_rate.0 as f64);
            let mut available = left.samples_avail();
            if available > n {
                available = n;
            }
            left.read_samples(buf_left, available, channels == 2);
            available
        };

        if channels == 2 {
            let mut core = core.as_mut();
            let mut right = core.audio_channel(1);
            right.set_rates(clock_rate as f64, sample_rate.0 as f64);
            right.read_samples(&mut buf_left[1..], available, channels == 2);
        }

        buf_left = &mut buf_left[(available * 2) as usize..];
        core.as_mut().run_frame();
    }

    buf.len()
}

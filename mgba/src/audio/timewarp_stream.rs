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

    let mut faux_clock = 1.0;
    if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
        sync.lock_audio();
        faux_clock = crate::gba::audio_calculate_ratio(1.0, sync.as_ref().fps_target(), 1.0);
    }

    let n = frame_count as i32;

    let available = {
        let mut core = core.as_mut();
        let mut left = core.audio_channel(0);
        left.set_rates(clock_rate as f64, sample_rate.0 as f64 * faux_clock as f64);
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
        right.set_rates(clock_rate as f64, sample_rate.0 as f64 * faux_clock as f64);
        right.read_samples(&mut buf[1..], available, channels == 2);
    }

    if let Some(sync) = core.as_mut().gba_mut().sync_mut().as_mut() {
        sync.consume_audio();
    }

    available as usize * channels as usize
}

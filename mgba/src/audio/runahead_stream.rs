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

    todo!()
}

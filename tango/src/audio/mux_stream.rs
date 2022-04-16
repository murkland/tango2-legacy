pub struct MuxStream {
    streams: Vec<Box<dyn super::Stream + Send + 'static>>,
    index: usize,
}

impl MuxStream {
    pub fn new(streams: Vec<Box<dyn super::Stream + Send + 'static>>) -> MuxStream {
        Self { streams, index: 0 }
    }
}

impl super::Stream for MuxStream {
    fn fill(&mut self, buf: &mut [i16]) -> usize {
        for (i, stream) in self.streams.iter_mut().enumerate() {
            if i == self.index {
                continue;
            }
            stream.fill(buf);
        }
        self.streams[self.index].fill(buf)
    }

    fn set_sample_rate(&mut self, sample_rate: cpal::SampleRate) {
        for stream in &mut self.streams {
            stream.set_sample_rate(sample_rate)
        }
    }

    fn set_channels(&mut self, channels: u16) {
        for stream in &mut self.streams {
            stream.set_channels(channels)
        }
    }
}

pub struct MuxStream {
    streams: Vec<Box<dyn super::Stream + Send + 'static>>,
    index: usize,
}

impl MuxStream {
    pub fn new(streams: Vec<Box<dyn super::Stream + Send + 'static>>) -> MuxStream {
        Self { streams, index: 1 }
    }
}

impl super::Stream for MuxStream {
    fn fill(&self, buf: &mut [i16]) -> usize {
        for (i, stream) in self.streams.iter().enumerate() {
            if i == self.index {
                continue;
            }
            stream.fill(buf);
        }
        self.streams[self.index].fill(buf)
    }
}

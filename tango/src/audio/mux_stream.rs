#[derive(Clone)]
pub struct MuxHandle {
    index: usize,
    mux_index: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl MuxHandle {
    pub fn switch(&self) {
        self.mux_index
            .store(self.index, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct MuxStream {
    streams: Vec<Box<dyn super::Stream + Send + 'static>>,
    index: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl MuxStream {
    pub fn new() -> MuxStream {
        Self {
            streams: vec![],
            index: std::sync::Arc::new(0.into()),
        }
    }

    pub fn add(&mut self, stream: impl super::Stream + Send + 'static) -> MuxHandle {
        let index = self.streams.len();
        self.streams.push(Box::new(stream));
        MuxHandle {
            index,
            mux_index: self.index.clone(),
        }
    }
}

impl super::Stream for MuxStream {
    fn fill(&self, buf: &mut [i16]) -> usize {
        let index = self.index.load(std::sync::atomic::Ordering::Relaxed);
        for (i, stream) in self.streams.iter().enumerate() {
            if i == index {
                continue;
            }
            stream.fill(buf);
        }
        self.streams[index].fill(buf)
    }
}

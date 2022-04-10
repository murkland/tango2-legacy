pub struct Counter {
    marks: std::collections::VecDeque<std::time::Instant>,
}

impl Counter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: std::collections::VecDeque::with_capacity(window_size),
        }
    }

    pub fn mark(&mut self) {
        if self.marks.len() == self.marks.capacity() {
            self.marks.pop_front();
        }
        self.marks.push_back(std::time::Instant::now());
    }

    pub fn median_duration(&self) -> std::time::Duration {
        let mut durations = self
            .marks
            .iter()
            .zip(self.marks.iter().skip(1))
            .map(|(x, y)| *y - *x)
            .collect::<Vec<std::time::Duration>>();
        if durations.is_empty() {
            return std::time::Duration::ZERO;
        }
        let mid = durations.len() / 2;
        let (_, v, _) = durations.as_mut_slice().select_nth_unstable(mid);
        *v
    }
}

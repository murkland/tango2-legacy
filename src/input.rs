#[derive(Clone)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Option<[u8; 0x100]>,
}

pub struct Queue {
    notify: tokio::sync::Notify,
    queues: std::sync::Mutex<[std::collections::VecDeque<Input>; 2]>,
    local_player_index: u8,
    local_delay: u32,
}

impl Queue {
    pub fn new(size: usize, local_delay: u32, local_player_index: u8) -> Self {
        let notify = tokio::sync::Notify::new();
        notify.notify_waiters();
        Queue {
            notify,
            queues: std::sync::Mutex::new([
                std::collections::VecDeque::with_capacity(size),
                std::collections::VecDeque::with_capacity(size),
            ]),
            local_player_index,
            local_delay,
        }
    }

    pub async fn add_input(&mut self, player_index: u8, input: Input) {
        loop {
            self.notify.notified().await;

            let mut queues = self.queues.lock().unwrap();
            let queue = &mut queues[player_index as usize];
            if queue.len() == queue.capacity() {
                continue;
            }
            queue.push_back(input);
            return;
        }
    }

    pub async fn queue_length(&self, player_index: u8) -> usize {
        let queues = self.queues.lock().unwrap();
        queues[player_index as usize].len()
    }

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<[Input; 2]>, Vec<Input>) {
        let mut queues = self.queues.lock().unwrap();

        let to_commit = {
            let mut n =
                queues[self.local_player_index as usize].len() as isize - self.local_delay as isize;
            if (queues[(1 - self.local_player_index) as usize].len() as isize) < n {
                n = queues[(1 - self.local_player_index) as usize].len() as isize;
            }

            if n < 0 {
                vec![]
            } else {
                let [ref mut q0, ref mut q1] = &mut *queues;
                let p0 = q0.drain(..n as usize);
                let p1 = q1.drain(..n as usize);
                p0.zip(p1).map(|(i0, i1)| [i0, i1]).collect()
            }
        };

        let peeked = {
            let n =
                queues[self.local_player_index as usize].len() as isize - self.local_delay as isize;
            if n < 0 {
                vec![]
            } else {
                queues[self.local_player_index as usize]
                    .range(..n as usize)
                    .cloned()
                    .collect()
            }
        };

        self.notify.notify_waiters();

        (to_commit, peeked)
    }
}

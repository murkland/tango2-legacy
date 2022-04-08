use std::time::Instant;

#[derive(Clone)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Option<[u8; 0x100]>,
}

pub struct Queue {
    condvar: parking_lot::Condvar,
    queues: parking_lot::Mutex<[std::collections::VecDeque<Input>; 2]>,
    local_player_index: u8,
    local_delay: u32,
}

impl Queue {
    pub fn new(size: usize, local_delay: u32, local_player_index: u8) -> Self {
        Queue {
            condvar: parking_lot::Condvar::new(),
            queues: parking_lot::Mutex::new([
                std::collections::VecDeque::with_capacity(size),
                std::collections::VecDeque::with_capacity(size),
            ]),
            local_player_index,
            local_delay,
        }
    }

    pub async fn add_input(
        &mut self,
        player_index: u8,
        input: Input,
        timeout: std::time::Duration,
    ) -> Result<(), anyhow::Error> {
        let deadline = Instant::now() + timeout;
        let mut queues = self.queues.lock();
        while queues[player_index as usize].len() == queues[player_index as usize].capacity() {
            if self.condvar.wait_until(&mut queues, deadline).timed_out() {
                anyhow::bail!("add input exceeded deadline")
            }
        }
        queues[player_index as usize].push_back(input);
        Ok(())
    }

    pub async fn queue_length(&self, player_index: u8) -> usize {
        let queues = self.queues.lock();
        queues[player_index as usize].len()
    }

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<[Input; 2]>, Vec<Input>) {
        let mut queues = self.queues.lock();

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

        self.condvar.notify_all();

        (to_commit, peeked)
    }
}

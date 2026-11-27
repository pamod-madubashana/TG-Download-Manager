use std::collections::VecDeque;
use std::sync::Mutex;

pub struct ChunkQueue {
    pending: Mutex<VecDeque<usize>>,
    attempts: Vec<Mutex<u32>>,
    max_attempts: u32,
}

impl ChunkQueue {
    pub fn new(indices: impl IntoIterator<Item = usize>, len: usize, max_attempts: u32) -> Self {
        Self {
            pending: Mutex::new(indices.into_iter().collect()),
            attempts: (0..len).map(|_| Mutex::new(0)).collect(),
            max_attempts,
        }
    }

    pub fn pop(&self) -> Option<usize> {
        self.pending.lock().unwrap().pop_front()
    }

    pub fn push_back(&self, idx: usize) {
        self.pending.lock().unwrap().push_back(idx);
    }

    pub fn retry(&self, idx: usize) -> bool {
        let mut attempts = self.attempts[idx].lock().unwrap();
        *attempts += 1;
        if *attempts >= self.max_attempts {
            false
        } else {
            self.push_back(idx);
            true
        }
    }

    pub fn attempts(&self, idx: usize) -> u32 {
        *self.attempts[idx].lock().unwrap()
    }

    pub fn len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

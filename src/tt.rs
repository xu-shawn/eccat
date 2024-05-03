use crate::evaluate::{Eval, EVAL_INFINITY};
use assert_size::assert_size;
use cozy_chess::Move;

#[derive(Debug)]
pub struct TranspositionTable {
    table: Box<[Bucket]>,
    total_entries: usize,
    used_entries: usize,
}

impl TranspositionTable {
    pub fn new(mb_size: usize) -> Self {
        let bytes = mb_size * 1024 * 1024;
        let bucket_size = core::mem::size_of::<Bucket>();
        let total_buckets = bytes / bucket_size;

        let table = vec![Bucket::default(); total_buckets];

        Self {
            table: table.into_boxed_slice(),
            total_entries: total_buckets * Bucket::ENTRIES,
            used_entries: 0,
        }
    }

    pub fn probe(&self, key: u64) -> Option<&Entry> {
        if self.table.is_empty() {
            return None;
        }

        let index = usize::try_from(key).unwrap() % self.table.len();

        self.table[index]
            .entries
            .iter()
            .find(|&entry| entry.key == key)
    }

    pub fn insert(&mut self, entry: Entry) {
        if self.table.is_empty() {
            return;
        }

        let index = usize::try_from(entry.key).unwrap() % self.table.len();

        self.table[index].store(entry, &mut self.used_entries);
    }

    pub fn resize(&mut self, mb_size: usize) {
        *self = Self::new(mb_size);
    }

    pub fn hashfull(&self) -> u16 {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        if self.table.len() > 0 {
            ((self.used_entries as f64 / self.total_entries as f64) * 1000f64).floor() as u16
        } else {
            0
        }
    }

    pub fn clear(&mut self) {
        for bucket in self.table.iter_mut() {
            for entry in &mut bucket.entries {
                *entry = Entry::default();
            }
        }

        self.used_entries = 0;
    }
}

assert_size!(Bucket, 64);
assert_size!(Entry, 16);

#[derive(Clone, Copy, Debug, Default)]
struct Bucket {
    entries: [Entry; Self::ENTRIES],
}

impl Bucket {
    const ENTRIES: usize = 64 / core::mem::size_of::<Entry>();

    fn store(&mut self, entry: Entry, used_entries: &mut usize) {
        let mut lowest_depth_index = 0;
        let mut lowest_depth = self.entries[lowest_depth_index].depth;

        for i in 1..Self::ENTRIES {
            if self.entries[i].depth < lowest_depth {
                lowest_depth_index = i;
                lowest_depth = self.entries[i].depth;
            }
        }

        if self.entries[lowest_depth_index].depth == 0 {
            *used_entries += 1;
        }

        self.entries[lowest_depth_index] = entry;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    key: u64,
    depth: u8,
    flag: Flag,
    score: Eval,
    best_move: Option<Move>,
}

impl Entry {
    pub const fn new(
        key: u64,
        depth: u8,
        flag: Flag,
        score: Eval,
        best_move: Option<Move>,
    ) -> Self {
        Self {
            key,
            depth,
            flag,
            score,
            best_move,
        }
    }

    pub const fn get(
        &self,
        depth: u8,
        ply: u8,
        alpha: Eval,
        beta: Eval,
    ) -> (Option<Eval>, Option<Move>) {
        let mut value = None;

        if self.depth >= depth {
            match self.flag {
                Flag::Exact => {
                    let mut score = self.score;

                    if score > EVAL_INFINITY - 256 {
                        score -= ply as Eval;
                    } else if score < 256 - EVAL_INFINITY {
                        score += ply as Eval;
                    }

                    value = Some(score);
                }
                Flag::Alpha => {
                    if self.score <= alpha {
                        value = Some(alpha);
                    }
                }
                Flag::Beta => {
                    if self.score >= beta {
                        value = Some(beta);
                    }
                }
            }
        }

        (value, self.best_move)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Flag {
    #[default]
    Exact,
    Alpha,
    Beta,
}
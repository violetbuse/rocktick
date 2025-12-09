use std::{
    sync::Mutex,
    // time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
// use rand::Rng;
// use ulid::Ulid;

// struct LockFreeUlidGenerator {
//     last_timestamp: AtomicU64,
//     counter: AtomicU64,
// }

// impl LockFreeUlidGenerator {
//     const MAX_COUNTER: u64 = 0xFFFF_FFFF; // 32-bit counter

//     fn new() -> Self {
//         Self {
//             last_timestamp: AtomicU64::new(0),
//             counter: AtomicU64::new(0),
//         }
//     }

//     fn generate(&self) -> Ulid {
//         loop {
//             // current time in milliseconds
//             let now = SystemTime::now()
//                 .duration_since(UNIX_EPOCH)
//                 .expect("SystemTime before UNIX EPOCH")
//                 .as_millis() as u64;

//             let last = self.last_timestamp.load(Ordering::Relaxed);

//             if now > last {
//                 // Time has advanced, reset counter
//                 if self
//                     .last_timestamp
//                     .compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst)
//                     .is_ok()
//                 {
//                     let system_time = UNIX_EPOCH + Duration::from_millis(now);

//                     self.counter.store(0, Ordering::SeqCst);
//                     return Ulid::from_datetime_with_source(system_time, &mut rand::rng());
//                 }
//             } else {
//                 // Same millisecond, increment counter
//                 let cnt = self.counter.fetch_add(1, Ordering::SeqCst);
//                 if cnt < Self::MAX_COUNTER {
//                     // combine timestamp + counter for deterministic monotonic ULID
//                     let mut bytes = [0u8; 16];
//                     let ulid_timestamp = now.to_be_bytes();
//                     bytes[..6].copy_from_slice(&ulid_timestamp[2..8]); // 48 bits timestamp
//                     bytes[6..10].copy_from_slice(&(cnt as u32).to_be_bytes()); // 32-bit counter
//                     rand::rng().fill(&mut bytes[10..]); // remaining 48-bit random
//                     return Ulid::from_bytes(bytes);
//                 } else {
//                     // Wait for next millisecond
//                     std::thread::yield_now();
//                 }
//             }
//         }
//     }
// }

// static GENERATOR: Lazy<LockFreeUlidGenerator> = Lazy::new(LockFreeUlidGenerator::new);

static GENERATOR: Lazy<Mutex<ulid::Generator>> = Lazy::new(|| Mutex::new(ulid::Generator::new()));

pub fn generate(prefix: &str) -> String {
    // let id = GENERATOR.generate().to_string();

    // format!("{prefix}_{}", id)

    let mut generator = GENERATOR.lock().expect("Failed to unwrap ulid generator.");
    let id = generator
        .generate()
        .expect("Failed to generate non-overflowing ulid somehow.");

    format!("{prefix}_{}", id)
}

pub fn gen_for_time(prefix: &str, datetime: DateTime<Utc>) -> String {
    let id = ulid::Ulid::from_datetime(datetime.into());

    format!("{prefix}_{}", id)
}

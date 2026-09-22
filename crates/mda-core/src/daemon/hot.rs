//! Documents the user touched recently. The summarizer serves their sections first.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Recently changed documents, by relative path, with a time-to-live.
#[derive(Debug)]
pub struct HotSet {
    ttl: Duration,
    touched: HashMap<String, Instant>,
}

impl HotSet {
    /// A set whose entries expire `ttl` after their last touch.
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, touched: HashMap::new() }
    }

    /// Mark a document as just changed.
    pub fn touch(&mut self, rel_path: &str, at: Instant) {
        self.touched.insert(rel_path.to_owned(), at);
    }

    /// Live paths, most recently touched first. Expired entries are dropped on the way.
    pub fn snapshot(&mut self, now: Instant) -> Vec<String> {
        self.touched.retain(|_, at| now.duration_since(*at) < self.ttl);
        let mut v: Vec<(Instant, String)> =
            self.touched.iter().map(|(p, at)| (*at, p.clone())).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        v.into_iter().map(|(_, p)| p).collect()
    }

    /// Number of live entries (without expiring).
    pub fn len(&self) -> usize {
        self.touched.len()
    }

    /// `true` when nothing is hot.
    pub fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recent_first_and_expiry() {
        let mut h = HotSet::new(Duration::from_secs(10));
        let t0 = Instant::now();
        h.touch("a.md", t0);
        h.touch("b.md", t0 + Duration::from_secs(1));
        h.touch("a.md", t0 + Duration::from_secs(2));
        assert_eq!(h.snapshot(t0 + Duration::from_secs(3)), vec!["a.md", "b.md"]);
        assert_eq!(
            h.snapshot(t0 + Duration::from_secs(11) + Duration::from_millis(1)),
            vec!["a.md"]
        );
        assert_eq!(h.len(), 1);
        assert!(h.snapshot(t0 + Duration::from_secs(20)).is_empty());
        assert!(h.is_empty());
    }
}

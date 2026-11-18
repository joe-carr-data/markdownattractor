//! Change-impact review (ADR-0009): after a change to one section, which other sections are
//! possibly out of date?
//!
//! These parts need no model and no store:
//!
//! - [`changes`]: pair the sections of a file before and after, and say which changed, which
//!   are new, and which could not be paired (and why). Nothing ambiguous becomes a change.
//! - [`terms`]: the search terms a change yields, the replaced wording first.
//!
//! and the one that needs the store and still no model:
//!
//! - [`candidates`]: the other sections a change is compared against, from the raw full-text
//!   index and the documents' links, capped and counted.

pub mod candidates;
pub mod changes;
pub mod terms;

pub use candidates::{Candidate, Candidates, Finder, Limits, QueryTerm, Source};
pub use changes::{FileReview, NotReviewed, SectionChange, Side, Skip, compare};
pub use terms::{Role, Term, TermKind, terms_for};

//! Change-impact review (ADR-0009): after a change to one section, which other sections are
//! possibly out of date?
//!
//! This module holds the parts that need no model and no store:
//!
//! - [`changes`]: pair the sections of a file before and after, and say which changed, which
//!   are new, and which could not be paired (and why). Nothing ambiguous becomes a change.
//! - [`terms`]: the search terms a change yields, the replaced wording first.

pub mod changes;
pub mod terms;

pub use changes::{FileReview, NotReviewed, SectionChange, Side, Skip, compare};
pub use terms::{Role, Term, TermKind, terms_for};

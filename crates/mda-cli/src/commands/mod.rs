//! One module per subcommand. Each exposes `Args` and `run`, returns an `ExitCode`, and does
//! all presentation through [`crate::output`].

pub mod doctor;
pub mod parse;
pub mod schema;

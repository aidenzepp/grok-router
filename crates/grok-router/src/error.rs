//! Command-boundary failures.
//!
//! Domain errors stay in their modules. A command wraps one when that failure
//! becomes the process result. `Display` is the sentence that path already prints.

use crate::catalog::{CartridgeError, CatalogError, ResolveError};
use crate::config_edit::{RefuseReason, ReplaceError};
use crate::skill::SkillFailure;

/// One command failure. The variant name carries `Error` through the enum name.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A failure whose kind is `error`.
    #[error("{message}")]
    Command {
        /// The sentence this path already prints.
        message: String,
    },
    /// A clap or skill usage failure. Kind is `usage`.
    #[error("{message}")]
    Usage {
        /// The sentence this path already prints.
        message: String,
    },
    /// A skill encode failure whose kind is `io` and that is not a [`SkillFailure`].
    #[error("{message}")]
    Io {
        /// The sentence this path already prints.
        message: String,
    },
    /// A refused config read or edit. Kind is `error`.
    #[error("{message}")]
    Refused {
        /// The reason the command wrapped.
        #[source]
        reason: RefuseReason,
        /// The sentence this path already prints.
        message: String,
    },
    /// A file replace that did not finish. Kind is `error`.
    #[error("{message}")]
    Replace {
        /// The replace that failed.
        #[source]
        source: ReplaceError,
        /// The sentence this path already prints.
        message: String,
    },
    /// A catalog load or parse failure. Kind is `error`.
    #[error("{message}")]
    Catalog {
        /// The catalog error the command wrapped.
        #[source]
        source: CatalogError,
        /// The sentence this path already prints.
        message: String,
    },
    /// A catalog match that was not exactly one row. Kind is `error`.
    #[error("{message}")]
    Resolve {
        /// The match error the command wrapped.
        #[source]
        source: ResolveError,
        /// The sentence this path already prints.
        message: String,
    },
    /// A cartridge that was not selected. Kind is `error`.
    #[error("{message}")]
    Cartridge {
        /// The cartridge error the command wrapped.
        #[source]
        source: CartridgeError,
        /// The sentence this path already prints.
        message: String,
    },
    /// A skill failure. Kind is [`SkillFailure::kind`].
    #[error(transparent)]
    Skill(#[from] SkillFailure),
}

impl Error {
    /// A command failure whose kind is `error`.
    #[must_use]
    pub fn command(message: impl Into<String>) -> Self {
        Self::Command {
            message: message.into(),
        }
    }

    /// A usage failure. Kind is `usage`.
    #[must_use]
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    /// An encode failure whose kind is `io`.
    #[must_use]
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// Wrap a config refusal. `message` is the sentence already printed for it.
    #[must_use]
    pub fn refused(reason: RefuseReason, message: impl Into<String>) -> Self {
        Self::Refused {
            reason,
            message: message.into(),
        }
    }

    /// Wrap a file replace. `message` is the sentence already printed for it.
    #[must_use]
    pub fn replace(source: ReplaceError, message: impl Into<String>) -> Self {
        Self::Replace {
            source,
            message: message.into(),
        }
    }

    /// Wrap a catalog failure. `message` is the sentence already printed for it.
    #[must_use]
    pub fn catalog(source: CatalogError, message: impl Into<String>) -> Self {
        Self::Catalog {
            source,
            message: message.into(),
        }
    }

    /// Wrap a match failure. `message` is the sentence already printed for it.
    #[must_use]
    pub fn resolve(source: ResolveError, message: impl Into<String>) -> Self {
        Self::Resolve {
            source,
            message: message.into(),
        }
    }

    /// Wrap a cartridge failure. `message` is the sentence already printed for it.
    #[must_use]
    pub fn cartridge(source: CartridgeError, message: impl Into<String>) -> Self {
        Self::Cartridge {
            source,
            message: message.into(),
        }
    }

    /// Kind string this path already emits: `error`, `usage`, `io`, or `refused`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Command { .. }
            | Self::Refused { .. }
            | Self::Replace { .. }
            | Self::Catalog { .. }
            | Self::Resolve { .. }
            | Self::Cartridge { .. } => "error",
            Self::Usage { .. } => "usage",
            Self::Io { .. } => "io",
            Self::Skill(failure) => failure.kind(),
        }
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::command(message)
    }
}

use framework_lib::chromium_ec::EcError;

/// Errors that can occur while controlling the fan.
#[derive(Debug)]
pub enum FanError {
    WrongLedCount { expected: usize, actual: usize },
    Ec(EcError),
}

impl std::fmt::Display for FanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLedCount { expected, actual } => {
                write!(f, "expected {expected} LED colors, got {actual}")
            }
            Self::Ec(err) => write!(f, "EC error: {err:?}"),
        }
    }
}

impl std::error::Error for FanError {}

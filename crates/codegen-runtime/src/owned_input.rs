//! Opt-in ownership policy for hosts which can report input-close failures.
//! This does not change byte-only readers or turn `File::drop` into fallible close.
use std::{error::Error, fmt};

/// An already acquired source. Implementations bound their own reads and define
/// what successful closure means. `close` consumes the source and is called once
/// after every ordinary read/parse result, including errors. A parse callback
/// must not publish its result: publication starts only after this helper returns.
/// The opener remains responsible for cleaning up any partial acquisition before
/// it can supply an owned source to this helper.
pub trait OwnedInput {
    type ReadError;
    type CloseError;
    fn read_all(&mut self) -> Result<Vec<u8>, Self::ReadError>;
    fn close(self) -> Result<(), Self::CloseError>;
}

#[derive(Debug)]
pub enum OwnedInputFailure<R, P> {
    Read(R),
    Parse(P),
}
impl<R: fmt::Display, P: fmt::Display> fmt::Display for OwnedInputFailure<R, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "input read: {e}"),
            Self::Parse(e) => write!(f, "input parse: {e}"),
        }
    }
}
impl<R: Error + 'static, P: Error + 'static> Error for OwnedInputFailure<R, P> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}

#[derive(Debug)]
pub enum OwnedInputError<R, P, C> {
    Read(R),
    Parse(P),
    Close {
        error: C,
        prior: Option<OwnedInputFailure<R, P>>,
    },
}
impl<R, P, C> OwnedInputError<R, P, C> {
    /// A secondary failure never replaces the primary close error or its source
    /// chain. Inspect this separately when reporting an unsuccessful close.
    pub fn prior_failure(&self) -> Option<&OwnedInputFailure<R, P>> {
        match self {
            Self::Close { prior, .. } => prior.as_ref(),
            _ => None,
        }
    }
}
impl<R: fmt::Display, P: fmt::Display, C: fmt::Display> fmt::Display for OwnedInputError<R, P, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "input read: {e}"),
            Self::Parse(e) => write!(f, "input parse: {e}"),
            Self::Close { error, .. } => write!(f, "input close: {error}"),
        }
    }
}
impl<R: Error + 'static, P: Error + 'static, C: Error + 'static> Error
    for OwnedInputError<R, P, C>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Read(e) => e,
            Self::Parse(e) => e,
            Self::Close { error, .. } => error,
        })
    }
}

/// Read the complete source, parse while it remains owned, then close. A close
/// error is primary and retains any read/parse failure separately. No value
/// escapes before successful close. Panics are not caught: a read/parse panic
/// unwinds and drops the source without calling this trait's explicit close.
pub fn read_parse_close<S: OwnedInput, T, P>(
    mut source: S,
    parse: impl FnOnce(&[u8]) -> Result<T, P>,
) -> Result<T, OwnedInputError<S::ReadError, P, S::CloseError>> {
    let result = match source.read_all() {
        Ok(bytes) => parse(&bytes).map_err(OwnedInputFailure::Parse),
        Err(error) => Err(OwnedInputFailure::Read(error)),
    };
    if let Err(error) = source.close() {
        return Err(OwnedInputError::Close {
            error,
            prior: result.err(),
        });
    }
    result.map_err(|failure| match failure {
        OwnedInputFailure::Read(error) => OwnedInputError::Read(error),
        OwnedInputFailure::Parse(error) => OwnedInputError::Parse(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    struct Source {
        events: Rc<RefCell<Vec<&'static str>>>,
        read: bool,
        close: bool,
    }
    impl OwnedInput for Source {
        type ReadError = &'static str;
        type CloseError = &'static str;
        fn read_all(&mut self) -> Result<Vec<u8>, Self::ReadError> {
            self.events.borrow_mut().push("read");
            if self.read { Err("read") } else { Ok(vec![1]) }
        }
        fn close(self) -> Result<(), Self::CloseError> {
            self.events.borrow_mut().push("close");
            if self.close { Err("close") } else { Ok(()) }
        }
    }
    #[test]
    fn owned_input_result_precedence() {
        for read in [false, true] {
            for parse in [false, true] {
                for close in [false, true] {
                    let events = Rc::new(RefCell::new(vec![]));
                    let result = read_parse_close(
                        Source {
                            events: events.clone(),
                            read,
                            close,
                        },
                        |_| {
                            events.borrow_mut().push("parse");
                            if parse { Err("parse") } else { Ok(7) }
                        },
                    );
                    assert_eq!(
                        *events.borrow(),
                        if read {
                            vec!["read", "close"]
                        } else {
                            vec!["read", "parse", "close"]
                        }
                    );
                    match result {
                        Err(OwnedInputError::Close {
                            error: "close",
                            prior,
                        }) => {
                            assert!(close);
                            match prior {
                                Some(OwnedInputFailure::Read("read")) => assert!(read),
                                Some(OwnedInputFailure::Parse("parse")) => assert!(!read && parse),
                                None => assert!(!read && !parse),
                                other => panic!("unexpected prior {other:?}"),
                            }
                        }
                        Err(OwnedInputError::Read("read")) => assert!(read && !close),
                        Err(OwnedInputError::Parse("parse")) => assert!(parse && !read && !close),
                        Ok(7) => assert!(!read && !parse && !close),
                        other => panic!("unexpected {other:?}"),
                    }
                }
            }
        }
    }

    #[derive(Debug)]
    struct Leaf(&'static str);
    impl fmt::Display for Leaf {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }
    impl Error for Leaf {}
    #[derive(Debug)]
    struct ReadFault(Leaf);
    #[derive(Debug)]
    struct ParseFault(Leaf);
    #[derive(Debug)]
    struct CloseFault(Leaf);
    macro_rules! fault {
        ($name:ident) => {
            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Display::fmt(&self.0, f)
                }
            }
            impl Error for $name {
                fn source(&self) -> Option<&(dyn Error + 'static)> {
                    Some(&self.0)
                }
            }
        };
    }
    fault!(ReadFault);
    fault!(ParseFault);
    fault!(CloseFault);
    struct TypedSource {
        read_failure: bool,
    }
    impl OwnedInput for TypedSource {
        type ReadError = ReadFault;
        type CloseError = CloseFault;
        fn read_all(&mut self) -> Result<Vec<u8>, ReadFault> {
            if self.read_failure {
                Err(ReadFault(Leaf("read-leaf")))
            } else {
                Ok(vec![])
            }
        }
        fn close(self) -> Result<(), CloseFault> {
            Err(CloseFault(Leaf("close-leaf")))
        }
    }
    #[test]
    fn close_stays_primary_with_typed_secondary_source_chains() {
        for read_failure in [false, true] {
            let error = read_parse_close(TypedSource { read_failure }, |_| {
                Err::<(), _>(ParseFault(Leaf("parse-leaf")))
            })
            .unwrap_err();
            let close = error
                .source()
                .unwrap()
                .downcast_ref::<CloseFault>()
                .unwrap();
            assert_eq!(
                close.source().unwrap().downcast_ref::<Leaf>().unwrap().0,
                "close-leaf"
            );
            let prior = error.prior_failure().unwrap();
            if read_failure {
                assert!(matches!(prior, OwnedInputFailure::Read(_)));
                let read = prior.source().unwrap().downcast_ref::<ReadFault>().unwrap();
                assert_eq!(
                    read.source().unwrap().downcast_ref::<Leaf>().unwrap().0,
                    "read-leaf"
                );
            } else {
                assert!(matches!(prior, OwnedInputFailure::Parse(_)));
                let parse = prior
                    .source()
                    .unwrap()
                    .downcast_ref::<ParseFault>()
                    .unwrap();
                assert_eq!(
                    parse.source().unwrap().downcast_ref::<Leaf>().unwrap().0,
                    "parse-leaf"
                );
            }
        }
    }
}

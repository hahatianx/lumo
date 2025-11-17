use std::fmt::{Debug, Display};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExpectOrNone<T: PartialEq> {
    Value(T),
    Any,
}

impl<T: PartialEq> From<Option<T>> for ExpectOrNone<T> {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => Self::Value(v),
            None => Self::Any,
        }
    }
}

impl<T: PartialEq> Into<Option<T>> for ExpectOrNone<T> {
    fn into(self) -> Option<T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Any => None,
        }
    }
}

impl<T: Debug + PartialEq> Debug for ExpectOrNone<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Value(v) => write!(f, "Expected({:?})", v),
            Self::Any => write!(f, "Not specified"),
        }
    }
}

impl<T: Display + PartialEq> Display for ExpectOrNone<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Value(v) => write!(f, "{}", v),
            Self::Any => write!(f, "Not specified"),
        }
    }
}

impl<T: PartialEq> ExpectOrNone<T> {
    pub fn has_expected(&self) -> bool {
        matches!(self, ExpectOrNone::Value(_))
    }

    pub fn is_any(&self) -> bool {
        matches!(self, ExpectOrNone::Any)
    }

    pub fn match_expected(&self, v: &T) -> bool {
        match self {
            ExpectOrNone::Value(v2) => v2 == v,
            ExpectOrNone::Any => true,
        }
    }

    pub fn not_match_expected(&self, v: &T) -> bool {
        !self.match_expected(v)
    }
}
pub type Expected<T> = ExpectOrNone<T>;

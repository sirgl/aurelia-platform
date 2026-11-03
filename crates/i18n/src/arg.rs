//! Arguments of a message: the named values a template reads.

/// A value a message can insert or select on.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i64),
    Float(f64),
    Text(String),
    /// A calendar date (year, month, day) for `{ DATE($d) }`.
    Date(i32, u8, u8),
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg { fn from(v: $t) -> Arg { Arg::Int(v as i64) } }
        impl From<&$t> for Arg { fn from(v: &$t) -> Arg { Arg::Int(*v as i64) } }
    )*};
}
from_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl From<f32> for Arg {
    fn from(v: f32) -> Arg {
        Arg::Float(v as f64)
    }
}
impl From<f64> for Arg {
    fn from(v: f64) -> Arg {
        Arg::Float(v)
    }
}
impl From<&str> for Arg {
    fn from(v: &str) -> Arg {
        Arg::Text(v.to_owned())
    }
}
impl From<&&str> for Arg {
    fn from(v: &&str) -> Arg {
        Arg::Text((*v).to_owned())
    }
}
impl From<String> for Arg {
    fn from(v: String) -> Arg {
        Arg::Text(v)
    }
}
impl From<&String> for Arg {
    fn from(v: &String) -> Arg {
        Arg::Text(v.clone())
    }
}
impl From<std::borrow::Cow<'_, str>> for Arg {
    fn from(v: std::borrow::Cow<'_, str>) -> Arg {
        Arg::Text(v.into_owned())
    }
}
impl From<bool> for Arg {
    /// `true` and `false`: a select can read them as `[true]` / `[false]`.
    fn from(v: bool) -> Arg {
        Arg::Text(v.to_string())
    }
}

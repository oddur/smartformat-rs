use std::collections::BTreeMap;

/// A value that can be substituted into a template.
///
/// This is the port's replacement for .NET reflection: instead of the
/// formatter reflecting over arbitrary objects, callers convert their data
/// into a `Value` tree — by hand, via the `From` impls, or with
/// `#[derive(ToSmartValue)]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    /// A date/time without timezone, like a .NET `DateTime` of unspecified
    /// kind. Zoned datetimes (`DateTimeOffset`) may come later.
    #[cfg(feature = "time")]
    DateTime(jiff::civil::DateTime),
}

/// Conversion into a [`Value`], implemented by hand or via
/// `#[derive(ToSmartValue)]`.
pub trait ToSmartValue {
    fn to_smart_value(&self) -> Value;
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

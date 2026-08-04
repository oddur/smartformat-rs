use std::collections::{BTreeMap, HashMap};

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
    /// An unsigned value that does not fit [`Value::Int`], mirroring a .NET
    /// `ulong`: it formats exactly, including with `D` and `X`.
    UInt(u64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    /// A date/time without timezone, like a .NET `DateTime` of unspecified
    /// kind. Zoned datetimes (`DateTimeOffset`) may come later.
    #[cfg(feature = "time")]
    DateTime(jiff::civil::DateTime),
    /// A duration, like a .NET `TimeSpan`. .NET ticks are 100 ns; a
    /// [`jiff::SignedDuration`] counts whole nanoseconds, so every `TimeSpan`
    /// is representable exactly.
    #[cfg(feature = "time")]
    TimeSpan(jiff::SignedDuration),
}

/// Conversion into a [`Value`], implemented by hand or via
/// `#[derive(ToSmartValue)]`.
pub trait ToSmartValue {
    fn to_smart_value(&self) -> Value;
}

impl ToSmartValue for Value {
    fn to_smart_value(&self) -> Value {
        self.clone()
    }
}

impl<T: ToSmartValue + ?Sized> ToSmartValue for &T {
    fn to_smart_value(&self) -> Value {
        (**self).to_smart_value()
    }
}

macro_rules! impl_via_into_i64 {
    ($($t:ty),* $(,)?) => {$(
        impl ToSmartValue for $t {
            fn to_smart_value(&self) -> Value {
                Value::Int(i64::from(*self))
            }
        }

        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::Int(i64::from(v))
            }
        }
    )*};
}

impl_via_into_i64!(i8, i16, i32, i64, u8, u16, u32);

// isize is at most 64 bits wide on every Rust target, so this never truncates.
impl ToSmartValue for isize {
    fn to_smart_value(&self) -> Value {
        Value::Int(*self as i64)
    }
}

impl From<isize> for Value {
    fn from(v: isize) -> Self {
        Value::Int(v as i64)
    }
}

/// Unsigned 64-bit values above [`i64::MAX`] keep their exact value in
/// [`Value::UInt`]; everything else is an ordinary [`Value::Int`], so the two
/// variants never represent the same number two ways.
fn u64_to_value(v: u64) -> Value {
    match i64::try_from(v) {
        Ok(i) => Value::Int(i),
        Err(_) => Value::UInt(v),
    }
}

macro_rules! impl_via_u64 {
    ($($t:ty),* $(,)?) => {$(
        impl ToSmartValue for $t {
            fn to_smart_value(&self) -> Value {
                u64_to_value(*self as u64)
            }
        }

        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                u64_to_value(v as u64)
            }
        }
    )*};
}

impl_via_u64!(u64, usize);

// f32 widens to f64 exactly, so a value that was inexact as f32 (0.1f32)
// renders with the artifacts of its f64 widening, not as ".NET float".
impl ToSmartValue for f32 {
    fn to_smart_value(&self) -> Value {
        Value::Float(f64::from(*self))
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(f64::from(v))
    }
}

impl ToSmartValue for f64 {
    fn to_smart_value(&self) -> Value {
        Value::Float(*self)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl ToSmartValue for bool {
    fn to_smart_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl ToSmartValue for char {
    fn to_smart_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl From<char> for Value {
    fn from(v: char) -> Self {
        Value::String(v.to_string())
    }
}

impl ToSmartValue for str {
    fn to_smart_value(&self) -> Value {
        Value::String(self.to_owned())
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl ToSmartValue for String {
    fn to_smart_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl<T: ToSmartValue> ToSmartValue for Option<T> {
    fn to_smart_value(&self) -> Value {
        match self {
            Some(v) => v.to_smart_value(),
            None => Value::Null,
        }
    }
}

impl<T: ToSmartValue> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.to_smart_value()
    }
}

impl<T: ToSmartValue> ToSmartValue for Vec<T> {
    fn to_smart_value(&self) -> Value {
        Value::List(self.iter().map(ToSmartValue::to_smart_value).collect())
    }
}

impl<T: ToSmartValue> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        v.to_smart_value()
    }
}

impl<T: ToSmartValue> ToSmartValue for [T] {
    fn to_smart_value(&self) -> Value {
        Value::List(self.iter().map(ToSmartValue::to_smart_value).collect())
    }
}

impl<T: ToSmartValue> ToSmartValue for BTreeMap<String, T> {
    fn to_smart_value(&self) -> Value {
        Value::Map(
            self.iter()
                .map(|(k, v)| (k.clone(), v.to_smart_value()))
                .collect(),
        )
    }
}

impl<T: ToSmartValue> From<BTreeMap<String, T>> for Value {
    fn from(v: BTreeMap<String, T>) -> Self {
        v.to_smart_value()
    }
}

impl<T: ToSmartValue, S> ToSmartValue for HashMap<String, T, S> {
    fn to_smart_value(&self) -> Value {
        Value::Map(
            self.iter()
                .map(|(k, v)| (k.clone(), v.to_smart_value()))
                .collect(),
        )
    }
}

impl<T: ToSmartValue, S> From<HashMap<String, T, S>> for Value {
    fn from(v: HashMap<String, T, S>) -> Self {
        v.to_smart_value()
    }
}

#[cfg(feature = "time")]
impl ToSmartValue for jiff::civil::DateTime {
    fn to_smart_value(&self) -> Value {
        Value::DateTime(*self)
    }
}

#[cfg(feature = "time")]
impl ToSmartValue for jiff::SignedDuration {
    fn to_smart_value(&self) -> Value {
        Value::TimeSpan(*self)
    }
}

#[cfg(feature = "time")]
impl From<jiff::SignedDuration> for Value {
    fn from(v: jiff::SignedDuration) -> Self {
        Value::TimeSpan(v)
    }
}

#[cfg(feature = "time")]
impl From<jiff::civil::DateTime> for Value {
    fn from(v: jiff::civil::DateTime) -> Self {
        Value::DateTime(v)
    }
}

/// The name .NET's error messages give the CLR type of a value, as
/// `Type.ToString()` spells it.
///
/// `null_name` is what [`Value::Null`] answers, because the two callers
/// disagree and each is quoting .NET verbatim: `PluralLocalizationFormatter`
/// names the type of the argument `Convert.ToDecimal` rejected, which is
/// `"null"`, while `TimeFormatter` interpolates `CurrentValue?.GetType()`,
/// which writes nothing at all for a null.
///
/// A map has no faithful counterpart: .NET quotes the CLR type name of
/// whatever dictionary was passed. We quote the type the goldens use,
/// `Dictionary<string, object>`.
///
/// Only `plural` and `time` name the type they were handed, so a build without
/// them has no caller.
#[cfg(feature = "plural")]
pub(crate) fn dotnet_type_name(value: &Value, null_name: &'static str) -> &'static str {
    match value {
        Value::Null => null_name,
        Value::Bool(_) => "System.Boolean",
        Value::Int(_) => "System.Int64",
        Value::UInt(_) => "System.UInt64",
        Value::Float(_) => "System.Double",
        Value::String(_) => "System.String",
        Value::List(_) => "System.Object[]",
        Value::Map(_) => "System.Collections.Generic.Dictionary`2[System.String,System.Object]",
        #[cfg(feature = "time")]
        Value::DateTime(_) => "System.DateTime",
        #[cfg(feature = "time")]
        Value::TimeSpan(_) => "System.TimeSpan",
    }
}

/// ```compile_fail
/// #[derive(smartformat::ToSmartValue)]
/// struct Tuple(i32);
/// ```
#[cfg(all(doctest, feature = "derive"))]
struct TupleStructIsRejected;

/// ```compile_fail
/// #[derive(smartformat::ToSmartValue)]
/// struct Unit;
/// ```
#[cfg(all(doctest, feature = "derive"))]
struct UnitStructIsRejected;

/// ```compile_fail
/// #[derive(smartformat::ToSmartValue)]
/// enum Choice {
///     One,
/// }
/// ```
#[cfg(all(doctest, feature = "derive"))]
struct EnumIsRejected;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_ints_become_int() {
        assert_eq!(i8::MIN.to_smart_value(), Value::Int(-128));
        assert_eq!(1i16.to_smart_value(), Value::Int(1));
        assert_eq!(1i32.to_smart_value(), Value::Int(1));
        assert_eq!(i64::MIN.to_smart_value(), Value::Int(i64::MIN));
        assert_eq!((-7isize).to_smart_value(), Value::Int(-7));
        assert_eq!(Value::from(-7i32), Value::Int(-7));
    }

    #[test]
    fn unsigned_ints_become_int() {
        assert_eq!(u8::MAX.to_smart_value(), Value::Int(255));
        assert_eq!(u16::MAX.to_smart_value(), Value::Int(65_535));
        assert_eq!(u32::MAX.to_smart_value(), Value::Int(4_294_967_295));
        assert_eq!(9usize.to_smart_value(), Value::Int(9));
        assert_eq!(Value::from(9u64), Value::Int(9));
    }

    #[test]
    fn u64_above_i64_max_stays_exact() {
        assert_eq!(
            (i64::MAX as u64).to_smart_value(),
            Value::Int(9_223_372_036_854_775_807)
        );
        assert_eq!(u64::MAX.to_smart_value(), Value::UInt(u64::MAX));
        assert_eq!(
            (i64::MAX as u64 + 1).to_smart_value(),
            Value::UInt(9_223_372_036_854_775_808)
        );
    }

    #[test]
    fn floats_bools_chars_and_strings() {
        assert_eq!(0.5f32.to_smart_value(), Value::Float(0.5));
        assert_eq!(0.5f64.to_smart_value(), Value::Float(0.5));
        assert_eq!(true.to_smart_value(), Value::Bool(true));
        assert_eq!('é'.to_smart_value(), Value::String("é".to_owned()));
        assert_eq!("hi".to_smart_value(), Value::String("hi".to_owned()));
        assert_eq!(
            String::from("hi").to_smart_value(),
            Value::String("hi".to_owned())
        );
    }

    #[test]
    fn references_forward_to_the_referent() {
        let owned = String::from("hi");
        let borrowed: &String = &owned;
        assert_eq!(borrowed.to_smart_value(), Value::String("hi".to_owned()));
        assert_eq!((&&1i32).to_smart_value(), Value::Int(1));
    }

    #[test]
    fn option_none_is_null() {
        assert_eq!(None::<i32>.to_smart_value(), Value::Null);
        assert_eq!(Some(3i32).to_smart_value(), Value::Int(3));
        assert_eq!(Value::from(None::<&str>), Value::Null);
    }

    #[test]
    fn vec_becomes_list() {
        assert_eq!(
            vec![1i32, 2, 3].to_smart_value(),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(Vec::<i32>::new().to_smart_value(), Value::List(vec![]));
        assert_eq!(
            vec![Some(1i32), None].to_smart_value(),
            Value::List(vec![Value::Int(1), Value::Null])
        );
    }

    #[test]
    fn maps_become_map_sorted_by_key() {
        let mut hash = HashMap::new();
        hash.insert("b".to_owned(), 2i32);
        hash.insert("a".to_owned(), 1i32);
        hash.insert("C".to_owned(), 3i32);

        let expected = Value::Map(BTreeMap::from([
            ("C".to_owned(), Value::Int(3)),
            ("a".to_owned(), Value::Int(1)),
            ("b".to_owned(), Value::Int(2)),
        ]));

        assert_eq!(hash.to_smart_value(), expected);

        let tree: BTreeMap<String, i32> = hash.into_iter().collect();
        assert_eq!(tree.to_smart_value(), expected);
        assert_eq!(Value::from(tree), expected);
    }

    #[test]
    fn nested_containers() {
        let mut outer = BTreeMap::new();
        outer.insert("xs".to_owned(), vec![1i32, 2]);
        assert_eq!(
            outer.to_smart_value(),
            Value::Map(BTreeMap::from([(
                "xs".to_owned(),
                Value::List(vec![Value::Int(1), Value::Int(2)])
            )]))
        );
    }

    #[test]
    fn value_converts_to_itself() {
        assert_eq!(Value::Null.to_smart_value(), Value::Null);
        assert_eq!(
            vec![Value::Int(1)].to_smart_value(),
            Value::List(vec![Value::Int(1)])
        );
    }

    #[cfg(feature = "time")]
    #[test]
    fn civil_datetime_becomes_datetime() {
        let dt = jiff::civil::date(2024, 3, 1).at(13, 45, 30, 0);
        assert_eq!(dt.to_smart_value(), Value::DateTime(dt));
        assert_eq!(Value::from(dt), Value::DateTime(dt));
    }
}

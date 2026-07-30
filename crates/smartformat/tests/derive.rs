#![cfg(feature = "derive")]
// Field names mirror .NET property names in templates, so they stay PascalCase.
#![allow(non_snake_case)]

use std::collections::BTreeMap;

use smartformat::value::{ToSmartValue, Value};
use smartformat::ToSmartValue;

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[derive(ToSmartValue)]
struct Address {
    City: String,
    Zip: u32,
}

#[derive(ToSmartValue)]
struct Person {
    Name: String,
    Age: i32,
    Address: Address,
    Nickname: Option<String>,
    Pets: Vec<Pet>,
}

#[derive(ToSmartValue)]
struct Pet {
    Name: String,
    Legs: u8,
}

#[test]
fn named_fields_become_a_map_keyed_by_field_name() {
    let person = Person {
        Name: "Ada".to_owned(),
        Age: 36,
        Address: Address {
            City: "London".to_owned(),
            Zip: 12345,
        },
        Nickname: None,
        Pets: vec![Pet {
            Name: "Kitty".to_owned(),
            Legs: 4,
        }],
    };

    assert_eq!(
        person.to_smart_value(),
        map([
            (
                "Address",
                map([
                    ("City", Value::String("London".to_owned())),
                    ("Zip", Value::Int(12345)),
                ])
            ),
            ("Age", Value::Int(36)),
            ("Name", Value::String("Ada".to_owned())),
            ("Nickname", Value::Null),
            (
                "Pets",
                Value::List(vec![map([
                    ("Legs", Value::Int(4)),
                    ("Name", Value::String("Kitty".to_owned())),
                ])])
            ),
        ])
    );
}

#[test]
fn option_some_unwraps_to_the_inner_value() {
    let person = Person {
        Name: "Ada".to_owned(),
        Age: 36,
        Address: Address {
            City: "London".to_owned(),
            Zip: 0,
        },
        Nickname: Some("Countess".to_owned()),
        Pets: Vec::new(),
    };

    let Value::Map(fields) = person.to_smart_value() else {
        panic!("expected a map");
    };
    assert_eq!(
        fields["Nickname"],
        Value::String("Countess".to_owned()),
        "Some(_) is transparent"
    );
    assert_eq!(fields["Pets"], Value::List(Vec::new()));
}

#[derive(ToSmartValue)]
struct CasePreserved {
    lower: i32,
    UPPER: i32,
    MixedCase: i32,
    r#type: i32,
}

#[test]
fn field_name_case_is_preserved_and_raw_identifiers_are_unraw() {
    let value = CasePreserved {
        lower: 1,
        UPPER: 2,
        MixedCase: 3,
        r#type: 4,
    };

    assert_eq!(
        value.to_smart_value(),
        map([
            ("MixedCase", Value::Int(3)),
            ("UPPER", Value::Int(2)),
            ("lower", Value::Int(1)),
            ("type", Value::Int(4)),
        ])
    );
}

#[derive(ToSmartValue)]
struct Empty {}

#[test]
fn a_struct_without_fields_becomes_an_empty_map() {
    assert_eq!(Empty {}.to_smart_value(), Value::Map(BTreeMap::new()));
}

#[derive(ToSmartValue)]
struct Wrapper<T> {
    Inner: T,
    Items: Vec<T>,
}

#[test]
fn generic_structs_derive_with_a_to_smart_value_bound() {
    let wrapper = Wrapper {
        Inner: 7i32,
        Items: vec![1i32, 2],
    };

    assert_eq!(
        wrapper.to_smart_value(),
        map([
            ("Items", Value::List(vec![Value::Int(1), Value::Int(2)])),
            ("Inner", Value::Int(7)),
        ])
    );
}

#[derive(ToSmartValue)]
struct Borrowing<'a> {
    Text: &'a str,
    Raw: Value,
}

#[test]
fn borrowed_and_raw_value_fields_convert() {
    let value = Borrowing {
        Text: "hi",
        Raw: Value::Bool(true),
    };

    assert_eq!(
        value.to_smart_value(),
        map([
            ("Raw", Value::Bool(true)),
            ("Text", Value::String("hi".to_owned())),
        ])
    );
}

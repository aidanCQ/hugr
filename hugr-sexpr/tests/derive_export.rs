use hugr_sexpr::{
    export::{export_values, Export},
    read_values, Value,
};

#[test]
pub fn positional() {
    #[derive(Export)]
    pub struct Test {
        first: String,
        second: String,
    }

    let test = Test {
        first: "a".into(),
        second: "b".into(),
    };

    let expected = read_values(r#""a" "b""#).unwrap();
    let exported: Vec<Value> = export_values(&test);

    assert_eq!(expected, exported);
}

#[test]
pub fn required() {
    #[derive(Export)]
    pub struct Test {
        first: String,
        #[sexpr(required)]
        required: String,
    }

    let test = Test {
        first: "a".into(),
        required: "b".into(),
    };

    let expected = read_values(r#""a" (required "b")"#).unwrap();
    let exported: Vec<Value> = export_values(&test);

    assert_eq!(expected, exported);
}

#[test]
pub fn optional_given() {
    #[derive(Export)]
    pub struct Test {
        first: String,
        #[sexpr(optional)]
        optional: Option<String>,
    }

    let test = Test {
        first: "a".into(),
        optional: Some("b".into()),
    };

    let expected = read_values(r#""a" (optional "b")"#).unwrap();
    let exported: Vec<Value> = export_values(&test);

    assert_eq!(expected, exported);
}

#[test]
pub fn optional_absent() {
    #[derive(Export)]
    pub struct Test {
        first: String,
        #[sexpr(optional)]
        optional: Option<String>,
    }

    let test = Test {
        first: "a".into(),
        optional: None,
    };

    let expected = read_values(r#""a""#).unwrap();
    let exported: Vec<Value> = export_values(&test);

    assert_eq!(expected, exported);
}

#[test]
pub fn repeated() {
    #[derive(Export)]
    struct Test {
        first: String,
        #[sexpr(repeated)]
        field: Vec<String>,
    }

    let mut test = Test {
        first: "a".into(),
        field: Vec::new(),
    };

    let mut expected_sexpr = r#""a""#.to_string();

    for i in 0..3 {
        let expected = read_values(&expected_sexpr).unwrap();
        let exported: Vec<Value> = export_values(&test);

        assert_eq!(expected, exported);

        test.field.push(format!("{}", i));
        expected_sexpr.push_str(&format!(r#" (field "{}")"#, i));
    }
}
